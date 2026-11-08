use super::*;
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::process::Command;
use std::thread::JoinHandle;

/// A child stopped after fork but before exec, when CLOEXEC has not acted yet.
struct BeforeExec {
    release: UnixStream,
    spawn: Option<JoinHandle<std::io::Result<std::process::Child>>>,
}

impl BeforeExec {
    fn spawn() -> Self {
        let (parent, child) = UnixStream::pair().unwrap();
        parent
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let fd = child.as_raw_fd();
        let spawn = std::thread::spawn(move || {
            let mut command = Command::new("/bin/sh");
            command.args(["-c", "exit 0"]);
            // SAFETY: only async-signal-safe syscalls run in the forked child.
            // poll bounds the pause even if the parent test fails to release it.
            unsafe {
                command.pre_exec(move || {
                    let ready = [1_u8];
                    if libc::write(fd, ready.as_ptr().cast(), 1) != 1 {
                        return Err(std::io::Error::last_os_error());
                    }
                    let mut poll = libc::pollfd {
                        fd,
                        events: libc::POLLIN,
                        revents: 0,
                    };
                    if libc::poll(&mut poll, 1, 10_000) != 1 {
                        return Err(std::io::Error::from_raw_os_error(libc::ETIMEDOUT));
                    }
                    let mut release = [0_u8];
                    if libc::read(fd, release.as_mut_ptr().cast(), 1) != 1 {
                        return Err(std::io::Error::from_raw_os_error(libc::EPIPE));
                    }
                    Ok(())
                });
            }
            let result = command.spawn();
            drop(child);
            result
        });
        let mut paused = Self {
            release: parent,
            spawn: Some(spawn),
        };
        let mut ready = [0_u8];
        if let Err(error) = paused.release.read_exact(&mut ready) {
            let _ = paused.finish();
            panic!("child did not reach pre-exec pause: {error}");
        }
        paused
    }

    fn finish(&mut self) -> std::io::Result<()> {
        if let Some(spawn) = self.spawn.take() {
            let _ = self.release.write_all(&[1]);
            let mut child = spawn
                .join()
                .map_err(|_| std::io::Error::other("spawn thread panicked"))??;
            if !child.wait()?.success() {
                return Err(std::io::Error::other("child exec failed"));
            }
        }
        Ok(())
    }
}

impl Drop for BeforeExec {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

#[test]
fn lifetime_lock_releases_before_child_exec_without_weakening_exclusion() {
    let (_root, data) = temp_data_root();
    let control = OverlayControl::acquire(&data, KEY).unwrap();
    let mut child = BeforeExec::spawn();
    assert!(matches!(
        OverlayControl::acquire(&data, KEY),
        Err(ControlError::AlreadyLocked(_))
    ));

    drop(control);
    let reacquired = OverlayControl::acquire(&data, KEY);
    if reacquired.is_ok() {
        assert!(matches!(
            OverlayControl::acquire(&data, KEY),
            Err(ControlError::AlreadyLocked(_))
        ));
    }
    // Always release/reap before asserting so the RED case cannot leak a child.
    child.finish().unwrap();
    assert!(
        reacquired.is_ok(),
        "a forked child must not retain the dropped owner's lock: {:?}",
        reacquired.err()
    );
    assert!(matches!(
        OverlayControl::acquire(&data, KEY),
        Err(ControlError::AlreadyLocked(_))
    ));
    drop(reacquired);
    OverlayControl::acquire(&data, KEY).unwrap();
}

#[test]
fn lifetime_lock_guard_releases_on_error_before_child_exec() {
    use super::super::acquire::{openat_file_nofollow, try_lock_exclusive};

    let (_root, data) = temp_data_root();
    let control = OverlayControl::acquire(&data, KEY).unwrap();
    let control_root = control.control_root().to_path_buf();
    drop(control);
    let dir = File::open(&control_root).unwrap();
    let file = openat_file_nofollow(dir.as_raw_fd(), "lifetime.lock", false).unwrap();
    let flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFD) };
    assert!(flags >= 0);
    assert_ne!(flags & libc::FD_CLOEXEC, 0, "CLOEXEC must remain atomic");
    let mut child = None;
    let failed_setup = (|| -> Result<(), ControlError> {
        let _lock = try_lock_exclusive(file, &control_root.join("lifetime.lock"))?;
        child = Some(BeforeExec::spawn());
        Err(ControlError::AuthenticationFailed)
    })();
    assert!(matches!(
        failed_setup,
        Err(ControlError::AuthenticationFailed)
    ));
    let reacquired = OverlayControl::acquire(&data, KEY);
    child.unwrap().finish().unwrap();
    assert!(
        reacquired.is_ok(),
        "failed setup must release the lock even before a child execs: {:?}",
        reacquired.err()
    );
}
