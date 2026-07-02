//! Sandbox drivers: spawn `openspine-shell` in isolation for each task.
//!
//! Two implementations are provided:
//! - [`ProcessDriver`]: spawns the shell binary as a child process with a
//!   cleared environment (dev / testing only — no network containment).
//! - [`DockerDriver`]: runs the shell image in a Docker container on an
//!   internal-only network (production — full egress containment).
//!
//! The kernel chooses a driver at startup from config and wraps it in
//! [`Sandbox`].  The kernel **must** refuse to route `external_communication`
//! events when the active driver is [`ProcessDriver`] unless
//! `unsafe_allow_uncontained_private_data: true` (decision O-003 / PRD §16).

use anyhow::Context as _;
use openspine_schemas::event::Lane;
use std::path::{Path, PathBuf};

// ──────────────────────────────────────────────────────────────────────────
// ProcessDriver
// ──────────────────────────────────────────────────────────────────────────

/// Spawns `openspine-shell` as a child process with a completely cleared
/// environment.  Only `KERNEL_ENDPOINT` and `TASK_TOKEN` are set on the
/// child — no ambient secrets, provider keys, or `OPENSPINE_*` vars leak.
///
/// **Dev/testing only.**  This driver provides no network or filesystem
/// isolation beyond the OS user boundary.  The kernel must block
/// `external_communication` events unless explicitly configured otherwise
/// (decision O-003).
#[derive(Debug, Clone)]
pub struct ProcessDriver {
    /// Path to the `openspine-shell` binary.  Defaults to `"openspine-shell"`
    /// (relies on `$PATH`).
    pub shell_binary: PathBuf,
    /// Root directory under which per-task scratch directories are created.
    /// Each task gets `<scratch_root>/<sanitized_task_token>/`.
    pub scratch_root: PathBuf,
}

impl Default for ProcessDriver {
    fn default() -> Self {
        Self {
            shell_binary: PathBuf::from("openspine-shell"),
            scratch_root: PathBuf::from("data/scratch"),
        }
    }
}

impl ProcessDriver {
    /// Build a `tokio::process::Command` for the shell invocation.
    ///
    /// The command has `env_clear()` applied before setting `KERNEL_ENDPOINT`
    /// and `TASK_TOKEN`, so no ambient env vars reach the child process.
    /// `scratch_dir` must already exist; the caller is responsible for creating
    /// it (see [`Self::run_task`]).
    fn build_command(
        &self,
        scratch_dir: &Path,
        kernel_endpoint: &str,
        task_token: &str,
    ) -> tokio::process::Command {
        let mut cmd = tokio::process::Command::new(&self.shell_binary);
        cmd.env_clear()
            .env("KERNEL_ENDPOINT", kernel_endpoint)
            .env("TASK_TOKEN", task_token)
            // CLI flags carry only the endpoint and token — never the
            // owner's message text, which would otherwise sit in argv,
            // readable via `ps`/`docker inspect` outside the shell's own
            // process. The shell fetches its pending message in-process
            // over the authenticated `GET /v1/task` call instead.
            .args(["--kernel", kernel_endpoint, "--task", task_token])
            .current_dir(scratch_dir);
        cmd
    }

    /// Spawn the shell binary for this task and wait for it to exit.
    ///
    /// Returns `Ok(())` on exit status 0, an error otherwise (non-zero exit,
    /// spawn failure, I/O error).  Never panics.
    pub async fn run_task(&self, kernel_endpoint: &str, task_token: &str) -> anyhow::Result<()> {
        // Derive a filesystem-safe directory name from the task token.
        let dir_name = task_token.replace(['/', '\\', '.', ':'], "_");
        let scratch_dir = self.scratch_root.join(&dir_name);
        std::fs::create_dir_all(&scratch_dir).with_context(|| {
            format!(
                "ProcessDriver: failed to create scratch dir {}",
                scratch_dir.display()
            )
        })?;

        let status = self
            .build_command(&scratch_dir, kernel_endpoint, task_token)
            .status()
            .await
            .with_context(|| {
                format!(
                    "ProcessDriver: failed to spawn shell binary {}",
                    self.shell_binary.display()
                )
            })?;

        if status.success() {
            Ok(())
        } else {
            Err(anyhow::anyhow!(
                "ProcessDriver: shell exited with non-zero status: {status}"
            ))
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────
// DockerDriver
// ──────────────────────────────────────────────────────────────────────────

/// Runs `openspine-shell` as an isolated Docker container via the `docker`
/// CLI (no shell interpolation — args are passed as a `Vec<String>`).
///
/// The container is attached to the configured internal-only network, runs
/// read-only with a tmpfs at `/tmp`, and starts as a non-root user.  The
/// only environment variables passed to the container are `KERNEL_ENDPOINT`
/// and `TASK_TOKEN`; Docker containers start with an empty env by default
/// unless explicitly populated, so no host secrets leak.
#[derive(Debug, Clone)]
pub struct DockerDriver {
    /// Docker image tag built from `Dockerfile.shell`.
    pub image_tag: String,
    /// Docker network name.  Must be an `internal: true` compose network so
    /// the shell container has no egress route to the public internet.
    pub network: String,
    /// Non-root UID passed to `docker run --user`.  Production default: 10001.
    pub run_as_uid: u32,
}

impl DockerDriver {
    /// Build the argument vector passed to `docker run` for this task.
    ///
    /// Pure function — no I/O, no shell interpolation.  Each argument is a
    /// separate `String`; the caller passes the slice directly to
    /// `Command::args`, so no shell quoting or injection is possible.
    pub fn docker_run_args(&self, kernel_endpoint: &str, task_token: &str) -> Vec<String> {
        vec![
            "run".to_string(),
            "--rm".to_string(),
            "--network".to_string(),
            self.network.clone(),
            "--read-only".to_string(),
            "--user".to_string(),
            self.run_as_uid.to_string(),
            "--tmpfs".to_string(),
            "/tmp".to_string(),
            // Only two env vars reach the container — no host env is forwarded.
            "-e".to_string(),
            format!("KERNEL_ENDPOINT={kernel_endpoint}"),
            "-e".to_string(),
            format!("TASK_TOKEN={task_token}"),
            // Image tag separates docker flags from container entrypoint args.
            self.image_tag.clone(),
            // Shell CLI flags passed after the image tag — never the
            // owner's message text (see ProcessDriver::build_command for
            // why: `docker inspect`/`ps` would otherwise leak it).
            "--kernel".to_string(),
            kernel_endpoint.to_string(),
            "--task".to_string(),
            task_token.to_string(),
        ]
    }

    /// Spawn the shell container for this task and wait for it to exit.
    ///
    /// Returns `Ok(())` on exit status 0, an error otherwise (non-zero exit,
    /// docker spawn failure, I/O error).  Never panics.
    pub async fn run_task(&self, kernel_endpoint: &str, task_token: &str) -> anyhow::Result<()> {
        let args = self.docker_run_args(kernel_endpoint, task_token);

        let status = tokio::process::Command::new("docker")
            .args(&args)
            .status()
            .await
            .context("DockerDriver: failed to invoke docker CLI")?;

        if status.success() {
            Ok(())
        } else {
            Err(anyhow::anyhow!(
                "DockerDriver: docker run exited with non-zero status: {status}"
            ))
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────
// Sandbox (top-level dispatcher)
// ──────────────────────────────────────────────────────────────────────────

/// The active sandbox driver, chosen at kernel startup from configuration.
#[derive(Debug)]
pub enum Sandbox {
    /// Child-process driver (dev/testing).  See [`ProcessDriver`].
    Process(ProcessDriver),
    /// Docker container driver (production).  See [`DockerDriver`].
    Docker(DockerDriver),
}

impl Sandbox {
    /// Spawn one shell invocation for this task and wait for it to finish.
    ///
    /// Returns `Ok(())` if the shell exited 0; returns an error on non-zero
    /// exit, spawn failure, or any I/O error. Never panics. The shell
    /// fetches its pending message itself over `GET /v1/task` — it is
    /// never passed here, so it never appears in argv/env visible via
    /// `ps`/`docker inspect`.
    pub async fn run_task(&self, kernel_endpoint: &str, task_token: &str) -> anyhow::Result<()> {
        match self {
            Sandbox::Process(d) => d.run_task(kernel_endpoint, task_token).await,
            Sandbox::Docker(d) => d.run_task(kernel_endpoint, task_token).await,
        }
    }
}

/// PRD §16 / decision O-003: the kernel **must** refuse to route an
/// `external_communication`-lane event when the active driver is
/// [`Sandbox::Process`], unless the operator explicitly opted in via
/// `unsafe_allow_uncontained_private_data: true`. `ProcessDriver` gives no
/// network/filesystem isolation beyond the OS user boundary (see its doc
/// comment) — spawning it for content bound for other people (an email
/// draft, a message reply) without that explicit opt-in would silently
/// widen the containment boundary the PRD promises. Pure/testable: the
/// pipeline calls this *before* composing authority or spawning a shell,
/// so a refusal here means the event never becomes a task grant at all.
pub fn refuses_external_communication_without_containment(
    lane: Lane,
    driver: &Sandbox,
    unsafe_allow_uncontained_private_data: bool,
) -> bool {
    lane == Lane::ExternalCommunication
        && matches!(driver, Sandbox::Process(_))
        && !unsafe_allow_uncontained_private_data
}

// ──────────────────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── containment refusal (O-003 / PRD §16) ────────────────────────────

    #[test]
    fn process_driver_refuses_external_communication_without_opt_in() {
        let driver = Sandbox::Process(ProcessDriver::default());
        assert!(refuses_external_communication_without_containment(
            Lane::ExternalCommunication,
            &driver,
            false
        ));
    }

    #[test]
    fn process_driver_allows_external_communication_with_explicit_opt_in() {
        let driver = Sandbox::Process(ProcessDriver::default());
        assert!(!refuses_external_communication_without_containment(
            Lane::ExternalCommunication,
            &driver,
            true
        ));
    }

    #[test]
    fn docker_driver_never_refuses_external_communication() {
        let driver = Sandbox::Docker(DockerDriver {
            image_tag: "openspine-shell:test".to_string(),
            network: "openspine-internal".to_string(),
            run_as_uid: 10001,
        });
        assert!(!refuses_external_communication_without_containment(
            Lane::ExternalCommunication,
            &driver,
            false
        ));
    }

    #[test]
    fn process_driver_never_refuses_owner_control_lane() {
        let driver = Sandbox::Process(ProcessDriver::default());
        assert!(!refuses_external_communication_without_containment(
            Lane::OwnerControl,
            &driver,
            false
        ));
    }

    // ── ProcessDriver: env isolation ─────────────────────────────────────

    /// Verify that `ProcessDriver` clears the environment before spawning the
    /// shell binary.  We substitute a tiny Unix shell script for the real
    /// `openspine-shell` binary and capture its stdout, which is a dump of the
    /// child process's environment.  A decoy `OPENSPINE_TEST_DECOY` var is set
    /// on the test process and must NOT appear in the child's env; only
    /// `KERNEL_ENDPOINT` and `TASK_TOKEN` must be present.
    ///
    /// Run with `--test-threads=1` (the acceptance command already does this)
    /// because `std::env::set_var` is not thread-safe.
    #[cfg(unix)]
    #[tokio::test]
    async fn process_driver_clears_env_and_sets_only_two_vars() {
        use std::os::unix::fs::PermissionsExt as _;

        let tmp = tempfile::tempdir().expect("tempdir");

        // Write a script that dumps its environment to stdout and exits 0.
        let script = tmp.path().join("print_env.sh");
        std::fs::write(&script, "#!/bin/sh\nenv\n").expect("write script");
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
            .expect("chmod +x");

        let scratch_root = tmp.path().join("scratch");
        std::fs::create_dir_all(&scratch_root).expect("create scratch_root");

        let driver = ProcessDriver {
            shell_binary: script,
            scratch_root: scratch_root.clone(),
        };

        // Plant a decoy that must NOT reach the child process.
        std::env::set_var("OPENSPINE_TEST_DECOY", "must-not-appear");

        let task_token = "tok-env-test-abc123";
        let kernel_ep = "http://127.0.0.1:7777";
        let scratch_dir = scratch_root.join(task_token);
        std::fs::create_dir_all(&scratch_dir).expect("create scratch_dir");

        // Use the private build_command helper (accessible because this test
        // module is a child of the module that defines ProcessDriver).
        let mut cmd = driver.build_command(&scratch_dir, kernel_ep, task_token);
        cmd.stdout(std::process::Stdio::piped());
        let output = cmd.output().await.expect("run env script");

        // Clean up decoy before assertions so a test panic doesn't leave it set.
        std::env::remove_var("OPENSPINE_TEST_DECOY");

        assert!(
            output.status.success(),
            "env script should exit 0; status: {}",
            output.status
        );

        let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");

        // Decoy must be absent — env_clear() must have fired.
        assert!(
            !stdout.contains("OPENSPINE_TEST_DECOY"),
            "OPENSPINE_ var leaked into child env:\n{stdout}"
        );
        // No other OPENSPINE_ or ANTHROPIC vars should be present either.
        assert!(
            !stdout.contains("OPENSPINE_"),
            "unexpected OPENSPINE_ var in child env:\n{stdout}"
        );
        assert!(
            !stdout.contains("ANTHROPIC"),
            "ANTHROPIC var leaked into child env:\n{stdout}"
        );

        // The two permitted vars must be present and exact.
        assert!(
            stdout
                .lines()
                .any(|l| l == format!("KERNEL_ENDPOINT={kernel_ep}")),
            "KERNEL_ENDPOINT missing or wrong in child env:\n{stdout}"
        );
        assert!(
            stdout
                .lines()
                .any(|l| l == format!("TASK_TOKEN={task_token}")),
            "TASK_TOKEN missing or wrong in child env:\n{stdout}"
        );
    }

    // ── DockerDriver: arg vector correctness ─────────────────────────────

    /// Verify that `DockerDriver::docker_run_args` produces the correct arg
    /// vector without actually invoking `docker`.  All containment flags must
    /// be present; no secret-looking string may appear.
    #[test]
    fn docker_driver_args_are_correct_and_secret_free() {
        let driver = DockerDriver {
            image_tag: "openspine-shell:latest".to_string(),
            network: "openspine-internal".to_string(),
            run_as_uid: 10001,
        };

        let kernel_ep = "http://kernel:7777";
        let task_tok = "my-task-token-xyz";

        let args = driver.docker_run_args(kernel_ep, task_tok);
        let joined = args.join(" ");

        // ── Required containment flags ────────────────────────────────────
        assert!(
            args.contains(&"--rm".to_string()),
            "--rm missing; args: {joined}"
        );
        assert!(
            args.contains(&"--read-only".to_string()),
            "--read-only missing; args: {joined}"
        );
        assert!(
            args.contains(&"--network".to_string()),
            "--network flag missing; args: {joined}"
        );
        assert!(
            args.contains(&"openspine-internal".to_string()),
            "network name missing; args: {joined}"
        );
        assert!(
            args.contains(&"--user".to_string()),
            "--user missing; args: {joined}"
        );
        assert!(
            args.contains(&"10001".to_string()),
            "uid 10001 missing; args: {joined}"
        );
        assert!(
            args.contains(&"--tmpfs".to_string()),
            "--tmpfs missing; args: {joined}"
        );
        assert!(
            args.contains(&"/tmp".to_string()),
            "/tmp missing; args: {joined}"
        );

        // ── Env vars: present and preceded by -e ─────────────────────────
        let ke_pos = args
            .iter()
            .position(|a| a == &format!("KERNEL_ENDPOINT={kernel_ep}"))
            .expect("KERNEL_ENDPOINT= arg missing");
        assert!(
            ke_pos > 0 && args[ke_pos - 1] == "-e",
            "KERNEL_ENDPOINT= must be preceded by -e; args: {joined}"
        );

        let tt_pos = args
            .iter()
            .position(|a| a == &format!("TASK_TOKEN={task_tok}"))
            .expect("TASK_TOKEN= arg missing");
        assert!(
            tt_pos > 0 && args[tt_pos - 1] == "-e",
            "TASK_TOKEN= must be preceded by -e; args: {joined}"
        );

        // ── Image tag present ─────────────────────────────────────────────
        assert!(
            args.contains(&"openspine-shell:latest".to_string()),
            "image tag missing; args: {joined}"
        );

        // ── Shell CLI flags present ──────────────────────────────────────
        assert!(
            args.contains(&"--kernel".to_string()),
            "--kernel missing; args: {joined}"
        );
        assert!(
            args.contains(&kernel_ep.to_string()),
            "kernel endpoint value missing; args: {joined}"
        );
        assert!(
            args.contains(&"--task".to_string()),
            "--task missing; args: {joined}"
        );
        assert!(
            args.contains(&task_tok.to_string()),
            "task token value missing; args: {joined}"
        );
        // ── The owner's message never appears in argv ────────────────────
        assert!(
            !args.contains(&"--message".to_string()),
            "--message must never be passed on argv (ps/docker inspect leak); args: {joined}"
        );

        // ── No secret-looking strings in any arg ─────────────────────────
        assert!(
            !joined.contains("OPENSPINE_"),
            "OPENSPINE_ var leaked into docker args: {joined}"
        );
        assert!(
            !joined.contains("ANTHROPIC"),
            "ANTHROPIC var leaked into docker args: {joined}"
        );
        // No --env-file or host env mounts
        assert!(
            !joined.contains("--env-file"),
            "--env-file must not appear; args: {joined}"
        );
    }
}
