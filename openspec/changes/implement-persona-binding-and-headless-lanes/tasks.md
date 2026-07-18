# Tasks

- [x] Additive `RouteWhen.channel_account` match and `Route.persona` reference to `openspine-schemas::route`.
- [x] Add `TaskGrant.persona_id` additive field threaded by `compose_authority` and the pipeline driver.
- [x] Implement pure `openspine-authority::persona_binding::resolve_persona` over (event channel account, resolved relationship, route, personas registry); binding derives from the route matched on channel+relationship, never agent input.
- [x] Implement kernel `pipeline::headless` lane: `run_headless_hook` verifies the bound MAC (payload/key/time/channel_account/action), mints the envelope, and drives `run_pipeline_with_envelope` with `spawn_shell=false` (no conversational shell), then dispatches the composed action through the headless mediation wrapper which preserves `ApprovalRequired` (standing rules cannot downgrade it). Surfacing is digest-only for no-approval; `ApprovalRequired` persists a resumable `ActionRequest` + owner approval button.
- [x] Bind a seeded persona (D-095) on the production owner-control route (`owner_telegram_main_assistant`) and add a regression that the seeded id resolves.
- [x] Add deterministic tests: owner/counterparty persona route binding, headless verified-hook no-approval zero-conversation digest-only flow, approval-required escalation, invalid/replayed/retargeted hook fails closed, cross-route replay namespace, capacity eviction, action-MAC-bind retarget rejection.
- [ ] Land through review, merge, and archive ceremony.
