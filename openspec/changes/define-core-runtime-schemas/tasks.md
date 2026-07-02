# Tasks: Define core runtime schemas

## 1. Create schema location

- [ ] Decide the initial schema location, for example `openspine/schemas/` or `schemas/`.
- [ ] Add a README explaining that schemas define runtime objects but do not activate authority.

## 2. Define event schemas

- [ ] Define `event_envelope`.
- [ ] Define source verification fields.
- [ ] Define replay protection fields.
- [ ] Define actor hints and target refs.
- [ ] Define lane and trust context fields.

## 3. Define identity and route schemas

- [ ] Define `identity_record`.
- [ ] Define `identity_resolution`.
- [ ] Define `route_artifact`.
- [ ] Define `route_resolution`.

## 4. Define authority schemas

- [ ] Define `authority_composition_input`.
- [ ] Define `authority_composition_result`.
- [ ] Define `task_grant`.
- [ ] Include allowed, denied, and approval-required actions.

## 5. Define action/approval/model/audit schemas

- [ ] Define `action_request`.
- [ ] Define `gate_decision`.
- [ ] Define `approval_record`.
- [ ] Define `selection_token`.
- [ ] Define `model_request`.
- [ ] Define `audit_event`.
- [ ] Define `artifact_ref`.

## 6. Verification

- [ ] Add examples for Telegram owner message and selected-thread email event.
- [ ] Add examples for owner-control task grant and selected-thread email task grant.
- [ ] Validate that identity records do not include live capability grants.
- [ ] Validate that route artifacts do not directly grant final authority.
- [ ] Validate that approval records bind payload and target digests.
- [ ] Validate that audit examples do not store private payloads as raw text.

## 7. Review

- [ ] Confirm consistency with `.raw/openspine-prd-v9.md`.
- [ ] Confirm consistency with `.raw/openspine-decision-log.md`.
- [ ] Run `openspec validate --changes define-core-runtime-schemas --strict`.
