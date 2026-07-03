# Spec: Shell containment

## Purpose

Contain the sandboxed `openspine-shell` process so it never holds ambient
secrets and, in production, has no route to arbitrary external services
(D-005, D-026, PRD §16 / decision O-003).

## ADDED Requirements

### Requirement: The shell environment MUST contain only KERNEL_ENDPOINT and TASK_TOKEN

Neither sandbox driver MUST forward any other environment variable
(provider API keys, the Telegram bot token, the artifact encryption key,
or any ambient host `OPENSPINE_*` variable) into the spawned shell.

#### Scenario: A shell process is spawned under ProcessDriver

Given the kernel spawns a shell process via `ProcessDriver`
When the child process's environment is inspected
Then it MUST contain exactly `KERNEL_ENDPOINT` and `TASK_TOKEN`
And no other variable MUST be present.

#### Scenario: A shell container is spawned under DockerDriver

Given the kernel spawns a shell container via `DockerDriver`
When the `docker run` invocation is inspected
Then only `KERNEL_ENDPOINT` and `TASK_TOKEN` MUST be passed via `-e`
And no host secret MUST appear in the argument vector.

(Enforced by `sandbox::tests::process_driver_clears_env_and_sets_only_two_vars`
and `sandbox::tests::docker_driver_args_are_correct_and_secret_free`.)

### Requirement: The Docker driver MUST provide no-public-egress networking, a read-only rootfs, and a non-root user

`DockerDriver` MUST attach the shell container only to the configured
internal-only Docker network (no route to the public internet), run its
root filesystem read-only, and run as a non-root user.

#### Scenario: A shell container is spawned in production

Given the kernel is configured with `sandbox.driver: docker`
When a shell container is spawned
Then its `docker run` arguments MUST include `--network <internal-network>`,
`--read-only`, and `--user <non-root-uid>`.

(Enforced by `sandbox::tests::docker_driver_args_are_correct_and_secret_free`;
the internal network's `internal: true` no-public-egress property is a
`compose.yaml` property, not independently assertable in `cargo test`.)

### Requirement: The kernel MUST refuse external-communication events under the Process driver without explicit opt-in

The kernel MUST refuse to route an `external_communication`-lane event
into a task grant when the active sandbox driver is `ProcessDriver` (no
network isolation), unless the operator has explicitly set
`unsafe_allow_uncontained_private_data: true`.

#### Scenario: An external-communication event arrives under ProcessDriver without opt-in

Given the active driver is `ProcessDriver`
And `unsafe_allow_uncontained_private_data` is `false`
When an `external_communication`-lane event arrives
Then the kernel MUST refuse to compose authority for it
And no task grant MUST be minted.

#### Scenario: The operator has explicitly opted in

Given the active driver is `ProcessDriver`
And `unsafe_allow_uncontained_private_data` is `true`
When an `external_communication`-lane event arrives
Then the kernel MAY proceed to compose authority for it.

#### Scenario: The Docker driver never needs the opt-in

Given the active driver is `DockerDriver`
When an `external_communication`-lane event arrives
Then the kernel MUST NOT refuse it on containment grounds regardless of
`unsafe_allow_uncontained_private_data`.

(Enforced by `sandbox::tests::process_driver_refuses_external_communication_without_opt_in`,
`sandbox::tests::process_driver_allows_external_communication_with_explicit_opt_in`,
`sandbox::tests::process_driver_never_refuses_owner_control_lane`, and
`sandbox::tests::docker_driver_never_refuses_external_communication`.)

### Requirement: The kernel↔shell transport trust assumption MUST be documented

This assumption MUST be written down, not left implicit: the
kernel↔shell HTTP link is plaintext (no TLS) and relies entirely on
running over a network with no route to the public internet for its
confidentiality.

#### Scenario: An implementer reviews the transport contract

Given `docs/kernel-http-contract.md`
When its "Transport trust assumption" section is read
Then it MUST state that the link is plaintext HTTP over the
compose-internal network, and that this is a deliberate, out-of-scope-for-TLS
decision for phases 1–3, not an oversight.
