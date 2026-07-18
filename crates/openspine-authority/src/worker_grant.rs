//! Runtime sub-grant minting for commissioned workers (AD-035 / AD-101).
//!
//! `mint_worker_grant` derives a child grant from a *parent* (the master
//! agent's own grant) as a Macaroons-style caveat chain: the child's MAC
//! extends the parent's already-sealed tip by folding one new
//! [`ChainStep`] onto it (`seal_child_from_parent_tip`), so verification
//! never needs a parent or root DB lookup — only the HMAC key and the
//! child's own embedded chain (offline-verifiable at the gate).
//!
//! The root-authority payload (`allowed_actions`, `approval_required_actions`,
//! `denied_actions`, `allowed_egress_classes`, `output_channels`, `limits`,
//! `expires_at`, `user`, `purpose`, `event_id`, `route_id`, `agent_id`,
//! `workflow_id`, `capability_pack_id`, `thread_id`) is authenticated as
//! part of that sealed tip and MUST stay byte-identical across every hop in
//! a chain (`grant_chain::RootAuthority`) — a child can therefore never
//! widen it, and cannot even independently choose a different
//! purpose/agent identity than its parent: a commissioned worker is the
//! SAME task identity as its master, running in an isolated shell with
//! strictly narrower attenuation. All actual narrowing is expressed as
//! caveats on the new hop instead:
//!
//! * `ActionAllowlist` — intersected with every prior hop's allowlist AND
//!   the root's own `allowed_actions` (never widens). Always added, even
//!   empty, so a worker commissioned with no actions is structurally locked
//!   out rather than silently inheriting the parent's full effective set.
//! * `BoundParameter` — AD-036 parameter locks; a later hop may add a name
//!   but never change an already-bound value.
//! * `ExpiresBefore` — the worker's grant can never outlive the parent's own
//!   effective expiry.
//! * `OutputChannelAllowlist { channels: vec![] }` — every commissioned
//!   worker gets this caveat unconditionally, so its *effective* output
//!   channels ([`grant_chain::effectively_allows_output_channel`]) are
//!   provably empty no matter what the root list carries: direct egress is
//!   structurally impossible, verifiable offline, without a DB lookup
//!   (AD-035 reply chokepoint). The worker's only way to communicate back
//!   is `worker.report_result`, which records a bus event; the master
//!   relays through its own separately-gated reply path.

use openspine_schemas::grant::TaskGrant;
use openspine_schemas::grant_chain::{self, Caveat, ChainStep};
use openspine_schemas::worker::WorkerCommissionSpec;
use rand::Rng;
use std::collections::BTreeMap;
use thiserror::Error;
use ulid::Ulid;

/// Why a worker sub-grant could not be minted.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum MintError {
    /// The parent grant's own chain did not verify against `key`.
    #[error("parent grant chain failed verification")]
    ParentChainInvalid,
    /// `spec.allowed_actions` named an action the parent authority does not
    /// effectively allow — a widening attempt.
    #[error("worker action {0} is not granted by the parent authority")]
    ActionNotInParentAuthority(String),
    /// `spec.expires_before` is later than the parent's effective expiry.
    #[error("worker expiry widens parent authority")]
    ExpiryWidens,
    /// `spec.bound_parameters` names a parameter the parent already bound to
    /// a different value.
    #[error("parameter {name} conflicts with an existing parent binding")]
    ParameterConflict { name: String },
}

/// Random bearer token for the freshly minted worker grant (transport
/// secret; redacted from outward serialization, D-032/D-047).
fn mint_task_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Mint a worker sub-grant from `parent` (the master agent's own grant),
/// attenuated per `spec`, sealed under HMAC `key`.
///
/// The returned grant is a valid child in `parent`'s chain: its MAC verifies
/// offline against `key` (no parent or root DB lookup), and it can never
/// exceed the parent's authority.
pub fn mint_worker_grant(
    parent: &TaskGrant,
    spec: &WorkerCommissionSpec,
    key: &[u8],
) -> Result<TaskGrant, MintError> {
    // The parent must itself be a valid, verified chain — a worker can only
    // ever attenuate real authority, never forge one.
    if !parent.verify_mac(key) {
        return Err(MintError::ParentChainInvalid);
    }

    // Narrowing checks against the parent's EFFECTIVE authority (root lists
    // intersected with the parent's own caveats).
    for action in &spec.allowed_actions {
        if !parent.effectively_allows(action) {
            return Err(MintError::ActionNotInParentAuthority(
                action.as_str().to_string(),
            ));
        }
    }

    // Effective parent expiry: grant expiry intersected with any
    // ExpiresBefore caveats the parent already carries.
    let mut parent_expiry = parent.expires_at;
    for caveat in grant_chain::flattened_caveats(&parent.chain) {
        if let Caveat::ExpiresBefore { at } = caveat {
            if *at < parent_expiry {
                parent_expiry = *at;
            }
        }
    }
    if spec.expires_before > parent_expiry {
        return Err(MintError::ExpiryWidens);
    }

    // Parent bindings the child must not contradict.
    let mut parent_bindings = BTreeMap::<String, String>::new();
    for caveat in grant_chain::flattened_caveats(&parent.chain) {
        if let Caveat::BoundParameter { name, value } = caveat {
            parent_bindings.insert(name.clone(), value.clone());
        }
    }
    for bp in &spec.bound_parameters {
        if let Some(existing) = parent_bindings.get(&bp.name) {
            if existing != &bp.value {
                return Err(MintError::ParameterConflict {
                    name: bp.name.clone(),
                });
            }
        }
    }

    // Build the new hop. The ActionAllowlist caveat is always added — even
    // empty — because omitting it when `spec.allowed_actions` is empty
    // would leave the worker's effective actions governed solely by the
    // parent's PRIOR caveats, i.e. as wide as the parent itself: a widening
    // bug for the exact case (no actions requested) that should be the
    // narrowest possible grant.
    let child_id = Ulid::new();
    let mut added_caveats: Vec<Caveat> = vec![Caveat::ActionAllowlist {
        actions: spec.allowed_actions.clone(),
    }];
    for bp in &spec.bound_parameters {
        added_caveats.push(Caveat::BoundParameter {
            name: bp.name.clone(),
            value: bp.value.clone(),
        });
    }
    added_caveats.push(Caveat::ExpiresBefore {
        at: spec.expires_before,
    });
    // The reply chokepoint: an empty OutputChannelAllowlist caveat narrows
    // the worker's effective output channels to the empty set regardless of
    // what the root carried (AD-035).
    added_caveats.push(Caveat::OutputChannelAllowlist { channels: vec![] });
    // The worker must also have zero AD-060 rated egress classes. This is a
    // separate caveat from output channels because the two policy axes are
    // intentionally independent at the gate.
    added_caveats.push(Caveat::EgressClassAllowlist { classes: vec![] });

    let child_step = ChainStep {
        grant_id: child_id,
        parent_grant_id: Some(parent.id),
        // Shadow is monotonic down a chain
        // (grant_chain::chain_structurally_valid rejects a live step after a
        // shadow one has been seen): inherit rather than hardcode Live so a
        // shadow master can only ever commission a shadow (non-executable)
        // worker, never escalate it to Live.
        mode: parent.mode,
        selection_tokens: vec![],
        added_caveats,
    };

    let caveat_mac = grant_chain::seal_child_from_parent_tip(&parent.caveat_mac, &child_step)
        .ok_or(MintError::ParentChainInvalid)?;

    // Every root-authority field is authenticated as part of the parent's
    // sealed tip and MUST stay byte-identical for the tip extension above
    // to verify — so clone the parent wholesale and touch only the fields
    // that legitimately vary per grant instance (identity, freshness,
    // lineage pointer, and the appended hop). `output_channels` is
    // therefore narrowed ONLY via the `OutputChannelAllowlist` caveat
    // above, never by mutating the field directly: a direct mutation would
    // desync this tip-extension from `verify_mac`'s independent recompute
    // (UNNUMBERED candidate: whether "structurally lack output_channels"
    // should instead mean a distinct wire type with no such field is
    // deferred — see IMPLEMENTATION-NOTES.md).
    let mut worker = parent.clone();
    worker.id = child_id;
    worker.issued_by = "kernel".to_string();
    worker.issued_at = jiff::Timestamp::now();
    worker.selection_tokens = vec![];
    worker.task_token = mint_task_token();
    worker.parent_grant_id = Some(parent.id);
    worker.chain.push(child_step);
    worker.caveat_mac = caveat_mac;

    // Self-check: the freshly minted child must verify offline under the
    // same key. If it does not, something in the construction is wrong and
    // we must not hand out an unverifiable grant.
    if !worker.verify_mac(key) {
        return Err(MintError::ParentChainInvalid);
    }
    Ok(worker)
}

#[cfg(test)]
mod worker_grant_tests {
    use super::*;
    use openspine_schemas::action::ActionId;
    use openspine_schemas::artifact::Lifecycle;
    use openspine_schemas::grant::{GrantLimits, GrantMode};
    use openspine_schemas::grant_chain;

    const TEST_KEY: &[u8] = b"openspine-test-grant-hmac-key-v1";

    fn root_grant() -> TaskGrant {
        let now = jiff::Timestamp::now();
        let mut grant = TaskGrant {
            id: Ulid::new(),
            schema_version: 1,
            lifecycle_state: Lifecycle::Active,
            user: "owner".to_string(),
            purpose: "master-task".to_string(),
            issued_by: "kernel".to_string(),
            issued_at: now,
            expires_at: now + std::time::Duration::from_secs(600),
            event_id: Ulid::new(),
            route_id: "owner_telegram_main_assistant".to_string(),
            agent_id: "main_assistant_agent".to_string(),
            workflow_id: "owner_control_conversation".to_string(),
            capability_pack_id: "owner_control_basic_pack".to_string(),
            authority_sources: vec![],
            selection_tokens: vec![],
            allowed_actions: vec![ActionId::new("openspine.status.read")],
            approval_required_actions: vec![],
            denied_actions: vec![],
            allowed_egress_classes: vec![],
            // Root carries a real output channel; a worker must still be
            // structurally choked to the empty set.
            output_channels: vec!["telegram:owner".to_string()],
            limits: GrantLimits {
                max_model_calls: 8,
                max_artifacts: 20,
                max_runtime_seconds: 120,
            },
            task_token: "root-token".to_string(),
            root_grant_id: Ulid::nil(),
            parent_grant_id: None,
            mode: GrantMode::Live,
            chain: vec![],
            caveat_mac: String::new(),
            thread_id: None,
            persona_id: None,
        };
        grant.root_grant_id = grant.id;
        grant.seal_root(TEST_KEY);
        grant
    }

    fn commission_spec(parent: &TaskGrant) -> WorkerCommissionSpec {
        WorkerCommissionSpec {
            agent_id: "worker_agent".to_string(),
            allowed_actions: parent.allowed_actions.clone(),
            bound_parameters: vec![],
            expires_before: parent.expires_at,
            purpose: "worker-task".to_string(),
            route_id: parent.route_id.clone(),
            workflow_id: parent.workflow_id.clone(),
            capability_pack_id: parent.capability_pack_id.clone(),
            counterparty_channel: None,
            counterparty_identifier: None,
            task_class: openspine_schemas::briefcase::TaskClass::Conversation,
        }
    }

    /// Offline chain verify: a multi-level caveat chain verifies against only
    /// the HMAC key + the child's embedded chain — no parent/root DB lookup.
    #[test]
    fn offline_chain_verify_multi_level() {
        let root = root_grant();
        assert!(root.verify_mac(TEST_KEY), "root must seal under the key");

        let worker = mint_worker_grant(&root, &commission_spec(&root), TEST_KEY)
            .expect("first-level mint must succeed");
        assert!(worker.verify_mac(TEST_KEY), "worker must verify offline");

        let deeper = mint_worker_grant(&worker, &commission_spec(&worker), TEST_KEY)
            .expect("second-level mint must succeed");
        assert!(
            deeper.verify_mac(TEST_KEY),
            "grandchild must verify offline"
        );

        // Nothing in the chain references a DB: verification is a pure MAC
        // recomputation over the embedded caveat chain.
        assert!(grant_chain::chain_structurally_valid(&deeper));
    }

    /// A worker cannot widen the parent's effective action authority.
    #[test]
    fn child_cannot_widen_parent_action() {
        let root = root_grant();
        let mut widen_actions = commission_spec(&root);
        widen_actions.allowed_actions = vec![ActionId::new("email.send_draft")];
        assert_eq!(
            mint_worker_grant(&root, &widen_actions, TEST_KEY),
            Err(MintError::ActionNotInParentAuthority(
                "email.send_draft".to_string()
            ))
        );
    }

    /// A worker cannot widen the parent's effective expiry.
    #[test]
    fn child_cannot_widen_parent_expiry() {
        let root = root_grant();
        let mut widen_expiry = commission_spec(&root);
        widen_expiry.expires_before = root.expires_at + std::time::Duration::from_secs(60);
        assert_eq!(
            mint_worker_grant(&root, &widen_expiry, TEST_KEY),
            Err(MintError::ExpiryWidens)
        );
        let narrow =
            mint_worker_grant(&root, &commission_spec(&root), TEST_KEY).expect("narrow spec mints");
        assert!(narrow.verify_mac(TEST_KEY));
    }

    /// Direct worker egress is impossible: the minted worker grant's
    /// *effective* output channels are provably empty (AD-035 reply
    /// chokepoint), verifiable offline without a DB lookup.
    #[test]
    fn direct_worker_egress_impossible() {
        let root = root_grant();
        let worker =
            mint_worker_grant(&root, &commission_spec(&root), TEST_KEY).expect("worker mints");
        // The empty OutputChannelAllowlist caveat wins, regardless of root:
        // effective output channels are provably empty (AD-035 reply
        // chokepoint), verifiable offline without a DB lookup.
        assert!(
            !grant_chain::effectively_allows_output_channel(&worker, "telegram:owner"),
            "worker must not inherit the root's output channel"
        );
    }

    #[test]
    fn worker_egress_class_is_narrowed() {
        let mut egress_root = root_grant();
        egress_root.allowed_egress_classes = vec![openspine_schemas::egress::EgressClass::Search];
        egress_root.seal_root(TEST_KEY);
        let egress_worker =
            mint_worker_grant(&egress_root, &commission_spec(&egress_root), TEST_KEY)
                .expect("worker with egress parent mints");
        assert!(
            !grant_chain::effectively_allows_egress_class(
                &egress_worker,
                &openspine_schemas::egress::EgressClass::Search
            ),
            "worker's empty egress caveat must narrow every parent egress class"
        );
    }
    /// Handoff caveat (Blocker 9-part): a worker minted from a root that
    /// carries a real output channel must still have *effectively empty*
    /// output channels. The worker's empty `OutputChannelAllowlist` caveat
    /// must NOT regain the parent's channel — direct egress stays structurally
    /// impossible even though the parent is allowed to egress (AD-035 reply
    /// chokepoint).
    #[test]
    fn minted_worker_has_empty_output_channels_despite_parent() {
        let root = root_grant();
        let worker =
            mint_worker_grant(&root, &commission_spec(&root), TEST_KEY).expect("worker mints");
        assert!(
            !grant_chain::effectively_allows_output_channel(&worker, "telegram:owner"),
            "worker's empty output-channel caveat must not regain the parent's channel"
        );
    }
}
