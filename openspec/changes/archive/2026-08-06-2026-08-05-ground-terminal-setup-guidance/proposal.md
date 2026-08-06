# Change: Ground the terminal assistant in the setup surface

## Layer

Lyra package (somatic). One prompt template artifact and one loader contract
test. No kernel code changes and no new runtime authority.

## Authority sensitivity

Not authority-sensitive. The template grants nothing; it adds owner-facing
guidance text. Denied tools, grant composition, and the action catalog are
unchanged.

## Why

On the gascity install, the owner asked the terminal assistant to set up
OAuth. Lyra answered: "Just need your client ID and secret or whatever
details you use." The prompt template gives the model no knowledge of the
CLI setup surface, so it improvises credential collection in chat.

That contradicts D-014 (bootstrap and setup secrets bypass shell and model
context): anything the owner pastes into chat enters model context and can
reach a hosted provider. The first-run onboarding change built the real
wizard, readiness report, and provider login; the assistant should route the
owner there instead of soliciting tokens it can never use.

## What Changes

- `owner_terminal_template` moves to version 2. The system preamble names
  the real setup surface (`openspine setup`, `openspine setup --check`,
  `openspine provider login anthropic`, and the in-chat `/status` and
  `/help` commands) and forbids asking for or accepting credentials in
  conversation, including exposure-and-rotate guidance when the owner pastes
  one anyway.
- A loader contract test pins the shipped template to that guidance.

## Acceptance Criteria

- Loading `artifacts/lyra` yields an `owner_terminal_template` whose
  preamble names `openspine provider login`, `openspine setup --check`, and
  `/status`, and instructs the assistant never to request or accept
  credentials in chat.
- `./scripts/check.sh 2026-08-05-ground-terminal-setup-guidance` passes.

## Out of Scope

- Wiring `setup.workflow.start` as a live dispatchable action. The agent
  manifest designs it; implementing it is its own change.
- Injecting live readiness state into the prompt. The `/status` command
  already reports it deterministically.
- Assertions on sampled model output. The contract binds the template
  artifact, the deterministic input, because model behavior is not a
  testable surface.
