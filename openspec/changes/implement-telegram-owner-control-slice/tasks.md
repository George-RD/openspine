# Tasks: Implement Telegram owner control slice

## 1. Telegram connector

- [ ] Add Telegram bot connector skeleton.
- [ ] Load Telegram bot token from environment or safe config.
- [ ] Load configured owner Telegram user ID.
- [ ] Implement polling or webhook receiver.

## 2. Event normalization

- [ ] Normalize owner messages into `telegram.owner.message`.
- [ ] Populate source verification fields.
- [ ] Populate lane and trust context.
- [ ] Deny or ignore unknown users.

## 3. Routing and authority

- [ ] Add owner Telegram route artifact.
- [ ] Add main assistant manifest.
- [ ] Add owner-control workflow manifest.
- [ ] Add owner-control capability pack.
- [ ] Compose owner-control task grant.

## 4. Actions

- [ ] Implement `openspine.status.read`.
- [ ] Implement `telegram.reply:owner_channel`.
- [ ] Stub `workflow.invoke:approved`.
- [ ] Stub `artifact.propose`.
- [ ] Stub `setup.workflow.start`.

## 5. Tests

- [ ] Test configured owner receives owner-control route.
- [ ] Test unknown Telegram user receives no owner authority.
- [ ] Test main assistant cannot read email inbox.
- [ ] Test main assistant cannot access raw network or host filesystem.
- [ ] Test Telegram reply is bound to owner chat.
- [ ] Test all effectful actions pass through gate().

## 6. Documentation

- [ ] Document Telegram setup.
- [ ] Document owner ID verification.
- [ ] Document unsafe dev shortcuts, if any.

## 7. Validation

- [ ] Run tests.
- [ ] Run `openspec validate --changes implement-telegram-owner-control-slice --strict`.
