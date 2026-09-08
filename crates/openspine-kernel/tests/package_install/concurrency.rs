//! A real overlap proof: B observes lock contention while A still holds it.
use super::Fixture;
use std::fs;
use std::path::Path;
use std::process::{Child, Output, Stdio};
use std::time::{Duration, Instant};

pub(super) fn contending_installers(fixture: &Fixture, second_source: &str) -> (Output, Output) {
    let barrier = tempfile::tempdir_in(fixture.root.path()).unwrap();
    let root = barrier.path();
    let mut first = Installer::spawn(fixture, "candidate", root, "lock-acquired");
    first.wait_ready(&root.join("lock-acquired.ready"));
    let mut second = Installer::spawn(fixture, second_source, root, "lock-contended");
    second.wait_ready(&root.join("lock-contended.ready"));
    first.assert_running();
    // A cannot finish before B observes contention. Hold B at that observation
    // until A finishes, so the production retry window is not a timing oracle.
    fs::write(root.join("lock-acquired.release"), []).unwrap();
    let first = first.output();
    fs::write(root.join("lock-contended.release"), []).unwrap();
    (first, second.output())
}

struct Installer(Option<Child>);

impl Installer {
    fn spawn(fixture: &Fixture, source: &str, barrier: &Path, point: &str) -> Self {
        Self(Some(
            fixture
                .command(&["install", "--from", source, "--json"])
                .env("OPENSPINE_TEST_PACKAGE_PAUSE", point)
                .env("OPENSPINE_TEST_PACKAGE_BARRIER", barrier)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        ))
    }

    fn assert_running(&mut self) {
        assert!(
            self.0.as_mut().unwrap().try_wait().unwrap().is_none(),
            "installer exited before the lock handshake"
        );
    }

    fn wait_ready(&mut self, path: &Path) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            self.assert_running();
            if path.is_file() {
                return;
            }
            assert!(Instant::now() < deadline, "missing installer readiness signal");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn output(mut self) -> Output {
        let deadline = Instant::now() + Duration::from_secs(10);
        while self.0.as_mut().unwrap().try_wait().unwrap().is_none() {
            assert!(Instant::now() < deadline, "installer did not finish");
            std::thread::sleep(Duration::from_millis(10));
        }
        self.0.take().unwrap().wait_with_output().unwrap()
    }
}

impl Drop for Installer {
    fn drop(&mut self) {
        // A failed assertion must not leave a child or data-directory lock behind.
        if let Some(child) = self.0.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
