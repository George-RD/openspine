//! A real pseudo-terminal for driving interactive consent. A TTY is only the
//! consent transport; nothing here supplies identity.
use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::{FromRawFd, OwnedFd};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub struct Session {
    child: Child,
    writer: File,
    output: Arc<Mutex<Vec<u8>>>,
    reader: Option<JoinHandle<()>>,
}

pub struct Finished {
    pub code: Option<i32>,
    pub output: String,
}

impl Session {
    pub fn spawn(mut command: Command) -> Self {
        let (mut master, mut slave) = (-1, -1);
        // SAFETY: openpty fills both descriptors on success; they are wrapped
        // exactly once below.
        assert_eq!(
            unsafe {
                libc::openpty(
                    &mut master,
                    &mut slave,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            },
            0
        );
        // SAFETY: the descriptors are valid, open and owned by this function.
        let (master, slave) =
            unsafe { (OwnedFd::from_raw_fd(master), OwnedFd::from_raw_fd(slave)) };
        command
            .stdin(Stdio::from(slave.try_clone().unwrap()))
            .stdout(Stdio::from(slave.try_clone().unwrap()))
            .stderr(Stdio::from(slave));
        let child = command.spawn().unwrap();
        // Release the parent's copies of the slave so the master sees hangup.
        drop(command);
        let writer = File::from(master.try_clone().unwrap());
        let mut reader_file = File::from(master);
        let output = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&output);
        let reader = std::thread::spawn(move || {
            let mut buffer = [0u8; 4096];
            // EIO (hangup) or zero ends the loop once the child has exited.
            while let Ok(read) = reader_file.read(&mut buffer) {
                if read == 0 {
                    break;
                }
                sink.lock().unwrap().extend_from_slice(&buffer[..read]);
            }
        });
        Self {
            child,
            writer,
            output,
            reader: Some(reader),
        }
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.output.lock().unwrap()).into_owned()
    }

    pub fn wait_for(&self, needle: &str) {
        let deadline = Instant::now() + Duration::from_secs(120);
        while !self.text().contains(needle) {
            assert!(
                Instant::now() < deadline,
                "never saw {needle:?}: {}",
                self.text()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    pub fn send_line(&mut self, line: &str) {
        self.writer
            .write_all(format!("{line}\n").as_bytes())
            .unwrap();
    }

    /// Ctrl-D at the start of a line: terminal end-of-file.
    pub fn send_eof(&mut self) {
        self.writer.write_all(&[0x04]).unwrap();
    }

    pub fn finish(mut self) -> Finished {
        let deadline = Instant::now() + Duration::from_secs(120);
        let status = loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                break status;
            }
            assert!(Instant::now() < deadline, "command hung: {}", self.text());
            std::thread::sleep(Duration::from_millis(20));
        };
        // Give the reader a moment to drain, then stop waiting for hangup.
        std::thread::sleep(Duration::from_millis(100));
        drop(self.reader.take());
        Finished {
            code: status.code(),
            output: self.text(),
        }
    }
}
