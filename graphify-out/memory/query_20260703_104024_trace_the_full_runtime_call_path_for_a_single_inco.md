---
type: "query"
date: "2026-07-03T10:40:24.357677+00:00"
question: "Trace the full runtime call path for a single incoming Telegram message from an authorized owner, all the way through to a TaskGrant being persisted in the kernel's SQLite store. List the modules/files involved, in call order, and identify the 2-3 modules that are most structurally central to this flow."
contributor: "graphify"
source_nodes: ["run_telegram_poll_loop()", ".poll_once()", "handle_owner_update()", "verify_update()", "build_owner_envelope()", "resolve_owner_identity()", "resolve_route()", "compose_authority()", "grant_with()", "request_for()"]
---

# Q: Trace the full runtime call path for a single incoming Telegram message from an authorized owner, all the way through to a TaskGrant being persisted in the kernel's SQLite store. List the modules/files involved, in call order, and identify the 2-3 modules that are most structurally central to this flow.

## Answer

Path: pipeline/mod.rs run_telegram_poll_loop() -> telegram.rs TelegramConnector.poll_once() -> pipeline/mod.rs handle_owner_update() -> telegram.rs verify_update()/build_owner_envelope() -> artifact_store.rs put() -> pipeline/mod.rs resolve_owner_identity() -> authority/route.rs resolve_route() -> authority/compose.rs compose_authority() (consults gate/gate.rs request_for()/grant_with() policy logic) -> store/mod.rs Store.insert_task_grant() (rusqlite Connection). Most central: pipeline/mod.rs handle_owner_update() (degree 33, orchestration hub), authority crate's compose.rs/route.rs (compose_authority degree 16, resolve_route degree 13), and gate/gate.rs (file degree 25, authority-decision core).

## Source Nodes

- run_telegram_poll_loop()
- .poll_once()
- handle_owner_update()
- verify_update()
- build_owner_envelope()
- resolve_owner_identity()
- resolve_route()
- compose_authority()
- grant_with()
- request_for()
- .insert_task_grant()
- TaskGrant