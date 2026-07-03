# model-gateway Specification

## Purpose
TBD - created by archiving change backfill-implemented-capability-specs. Update Purpose after archive.
## Requirements
### Requirement: Private-context model calls MUST be constructed kernel-side

The shell MUST submit a `model.generate` request describing its purpose
and message content; the kernel, not the shell, MUST resolve the prompt
template, build the final request, and make the provider call.

#### Scenario: Shell requests a model generation

Given the shell submits a `model.generate` request
When the kernel dispatches it
Then the kernel MUST resolve the agent's prompt template server-side
And the kernel MUST make the provider HTTP call itself
And the shell MUST NOT receive the provider's raw API credentials.

### Requirement: Provider credentials MUST never reach the shell

Provider API keys and OAuth tokens MUST remain kernel-side. The shell's
sandboxed environment MUST NOT contain them.

#### Scenario: Shell environment is inspected

Given a task's sandboxed shell process or container
When its environment variables are inspected
Then only `KERNEL_ENDPOINT` and `TASK_TOKEN` MUST be present
And no provider API key or OAuth credential MUST appear.

(Enforced by `sandbox::tests::process_driver_clears_env_and_sets_only_two_vars`
and `sandbox::tests::docker_driver_args_are_correct_and_secret_free`.)

### Requirement: Untrusted context MUST be wrapped with a per-call randomised delimiter

Untrusted external content included in a model call MUST be wrapped in a
delimited block using a delimiter minted fresh per call, prefixed with a
data-not-instruction preamble. A static or predictable delimiter MUST
NOT be used.

#### Scenario: Untrusted context contains a spoofed closing marker

Given untrusted context whose text contains what looks like a closing
delimiter
When the kernel builds the prompt
Then the spoofed marker MUST NOT be able to close the untrusted block
early
And the real (randomly minted) delimiter MUST still bound the untrusted
content correctly.

(Enforced by `model_gateway::tests::a_spoofed_closing_marker_in_the_content_does_not_escape_the_boundary`
and `model_gateway::tests::the_boundary_token_is_different_on_every_call`.)

### Requirement: Prompt templates MUST come from the kernel registry, never from shell input

The prompt template used for a `model.generate` call MUST be resolved
from the kernel's own artifact registry based on the requesting agent's
identity, never accepted as shell-supplied content.

#### Scenario: Shell requests generation for a known agent

Given a task grant with a known `agent_id`
When `model.generate` is dispatched
Then the kernel MUST look up that agent's template in its own registry
And the shell MUST have no way to substitute a different template.

### Requirement: Conversation state MUST store only role and artifact digest

Persisted conversation turns MUST record only the speaker's role and a
digest reference into the artifact store — never the raw message text.

#### Scenario: A conversation turn is persisted

Given a `model.generate` call appends a user or assistant turn
When the turn is stored
Then the stored row MUST contain only the role and an artifact digest
And the raw text MUST be recoverable only via the artifact store, not the
conversation table itself.

