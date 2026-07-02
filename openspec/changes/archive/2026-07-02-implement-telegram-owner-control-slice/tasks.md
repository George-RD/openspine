# Tasks: Implement Telegram owner control slice

## 1. Telegram connector

- [x] Add Telegram bot connector skeleton.
- [x] Load Telegram bot token from environment or safe config.
- [x] Load configured owner Telegram user ID.
- [x] Implement polling or webhook receiver.

## 2. Event normalization

- [x] Normalize owner messages into `telegram.owner.message`.
- [x] Populate source verification fields.
- [x] Populate lane and trust context.
- [x] Deny or ignore unknown users.

## 3. Routing and authority

- [x] Add owner Telegram route artifact.
- [x] Add main assistant manifest.
- [x] Add owner-control workflow manifest.
- [x] Add owner-control capability pack.
- [x] Compose owner-control task grant.

## 4. Actions

- [x] Implement `openspine.status.read`.
- [x] Implement `telegram.reply:owner_channel`.
- [x] Stub `workflow.invoke:approved`.
- [x] Stub `artifact.propose`.
- [x] Stub `setup.workflow.start`.

## 5. Tests

- [x] Test configured owner receives owner-control route.
- [x] Test unknown Telegram user receives no owner authority.
- [x] Test main assistant cannot read email inbox.
- [x] Test main assistant cannot access raw network or host filesystem.
- [x] Test Telegram reply is bound to owner chat.
- [x] Test all effectful actions pass through gate().

## 6. Documentation

- [x] Document Telegram setup.
- [x] Document owner ID verification.
- [x] Document unsafe dev shortcuts, if any.

## 7. Validation

- [x] Run tests. `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -- --test-threads=1 && bash scripts/check-file-sizes.sh` — 156 passed (10 suites), clippy clean, fmt clean, all files under 500 lines.
- [x] Run `openspec validate --changes implement-telegram-owner-control-slice --strict` — passed.
