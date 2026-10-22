//! Worker stderr capture shared by the sandbox drivers.
//!
//! The worker shell narrates its progress on stderr (`[openspine-shell] INFO:
//! …`). Inherited, those lines land in the owner's terminal in the middle of a
//! chat transcript. The drivers instead pipe stderr here: every line goes to
//! the kernel log at `debug` (target `openspine::worker`), and a bounded tail
//! is kept so a failed worker can be reported with its cause.

use std::collections::VecDeque;
use std::process::{ExitStatus, Stdio};
use std::time::Duration;

use tokio::io::{AsyncBufReadExt as _, AsyncReadExt as _, BufReader};

/// Lines of worker stderr kept for failure reports.
const TAIL_LINES: usize = 20;
/// Longest stretch of stderr treated as one line. The worker is untrusted, so
/// a newline-free stream must not grow the buffer without bound.
const MAX_LINE_BYTES: u64 = 1024;
/// How long to wait for the pipe to drain after the worker exited. A
/// grandchild that inherited the pipe must not stall the turn.
const DRAIN_GRACE: Duration = Duration::from_secs(2);

/// Run `command` to completion with its stderr captured. Returns the exit
/// status and the tail of stderr (oldest line first, newline-joined, control
/// characters replaced).
pub(super) async fn run_capturing_stderr(
    mut command: tokio::process::Command,
) -> std::io::Result<(ExitStatus, String)> {
    command.stderr(Stdio::piped());
    let mut child = command.spawn()?;
    let stderr = child.stderr.take();
    let mut reader = tokio::spawn(async move {
        let mut tail: VecDeque<String> = VecDeque::with_capacity(TAIL_LINES);
        let Some(stderr) = stderr else {
            return String::new();
        };
        let mut reader = BufReader::new(stderr);
        let mut buffer = Vec::new();
        loop {
            buffer.clear();
            // Always keep draining: a worker blocked on a full pipe would
            // never exit.
            match (&mut reader)
                .take(MAX_LINE_BYTES)
                .read_until(b'\n', &mut buffer)
                .await
            {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
            let line = sanitize(&String::from_utf8_lossy(&buffer));
            if line.is_empty() {
                continue;
            }
            tracing::debug!(target: "openspine::worker", "{line}");
            if tail.len() == TAIL_LINES {
                tail.pop_front();
            }
            tail.push_back(line);
        }
        tail.into_iter().collect::<Vec<_>>().join("\n")
    });
    let status = child.wait().await?;
    let tail = match tokio::time::timeout(DRAIN_GRACE, &mut reader).await {
        Ok(Ok(tail)) => tail,
        Ok(Err(_)) => String::new(),
        Err(_) => {
            reader.abort();
            String::new()
        }
    };
    Ok((status, tail))
}

/// One owner-readable line describing why the worker failed: the last line of
/// its stderr, or empty when it said nothing.
pub(super) fn failure_summary(stderr_tail: &str) -> &str {
    stderr_tail.lines().next_back().unwrap_or("")
}

fn sanitize(line: &str) -> String {
    line.trim_end_matches(['\r', '\n'])
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .trim()
        .to_string()
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn sh(script: &str) -> tokio::process::Command {
        let mut command = tokio::process::Command::new("sh");
        command.args(["-c", script]);
        command
    }

    #[tokio::test]
    async fn captures_status_and_stderr_tail_without_inheriting_stderr() {
        let (status, tail) = run_capturing_stderr(sh("echo one >&2; echo two >&2; exit 2"))
            .await
            .unwrap();
        assert_eq!(status.code(), Some(2));
        assert_eq!(tail, "one\ntwo");
        assert_eq!(failure_summary(&tail), "two");
    }

    #[tokio::test]
    async fn keeps_only_the_newest_lines_and_drains_a_flood() {
        // Far more than a pipe buffer holds: a reader that stopped draining
        // would deadlock the child here.
        let (status, tail) = run_capturing_stderr(sh(
            "i=0; while [ $i -lt 5000 ]; do echo \"line $i\" >&2; i=$((i+1)); done",
        ))
        .await
        .unwrap();
        assert!(status.success());
        let lines: Vec<_> = tail.lines().collect();
        assert_eq!(lines.len(), TAIL_LINES);
        assert_eq!(lines.last(), Some(&"line 4999"));
    }

    #[tokio::test]
    async fn bounds_a_newline_free_stream_and_strips_control_characters() {
        let (_, tail) = run_capturing_stderr(sh(
            "printf '\\033[31mred\\033[0m\\n'; head -c 100000 /dev/zero | tr '\\0' x >&2",
        ))
        .await
        .unwrap();
        assert!(!tail.contains('\u{1b}'), "{tail:?}");
        assert!(tail
            .lines()
            .all(|line| line.len() <= MAX_LINE_BYTES as usize));
    }

    #[tokio::test]
    async fn a_missing_binary_is_a_spawn_error() {
        let command = tokio::process::Command::new("/nonexistent/openspine-shell");
        assert!(run_capturing_stderr(command).await.is_err());
    }
}
