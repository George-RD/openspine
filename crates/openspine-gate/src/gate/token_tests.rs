use std::collections::HashMap;

use openspine_schemas::event::{AccountRole, Connector};
use openspine_schemas::selection::{
    SelectionScope, SelectionToken, SelectionTokenType, SelectionVerificationMethod,
};

use super::tests::{grant_with, request_for};
use super::*;

#[derive(Default)]
struct MockContext {
    tokens: HashMap<Ulid, SelectionToken>,
}

impl GateContext for MockContext {
    fn approval_for_request(&self, _action_request_id: Ulid) -> Option<ApprovalRecord> {
        None
    }

    fn find_selection_token(&self, id: Ulid) -> Option<SelectionToken> {
        self.tokens.get(&id).cloned()
    }

    fn grant_hmac_key(&self) -> Option<Vec<u8>> {
        Some(b"openspine-test-grant-hmac-key-v1".to_vec())
    }
}

fn grant_with_token(allowed: &[&str], token_id: Ulid) -> TaskGrant {
    let mut grant = grant_with(allowed, &[], &[]);
    grant.selection_tokens.push(token_id);
    // selection_tokens are MAC-bound; re-seal after binding.
    grant.seal_root(b"openspine-test-grant-hmac-key-v1");
    grant
}

fn request_for_token(action: &str, token_id: Ulid) -> ActionRequest {
    let mut req = request_for(action);
    req.selection_token_id = Some(token_id);
    req
}

fn make_token(id: Ulid, expires_at: Timestamp) -> SelectionToken {
    SelectionToken {
        id,
        schema_version: 1,
        token_type: SelectionTokenType::email_thread_selection(),
        user: "owner".to_string(),
        target_id: "thread_abc123".to_string(),
        selected_by: "owner".to_string(),
        selected_at: expires_at - std::time::Duration::from_secs(600),
        issued_by: "kernel".to_string(),
        expires_at,
        verified_source: true,
        verification_method: SelectionVerificationMethod::KernelUiSelection,
        connector: Some(Connector::GmailPrimaryConnector),
        account_role: Some(AccountRole::OwnerMailbox),
        scope: SelectionScope {
            read_thread: true,
            attachments_allowed: false,
            max_messages: 20,
            include_headers: true,
            include_recipients: true,
            include_body: true,
        },
        single_use: true,
    }
}

#[test]
fn kernel_origin_owner_notify_is_auto_allowed() {
    let grant = grant_with(&[], &[], &[]);
    let req = request_for("owner.notify");
    let ctx = MockContext::default();
    let outcome = gate(
        &grant,
        &req,
        ActionOrigin::Kernel,
        &ctx,
        &test_catalog(),
        &NoEgress,
        Timestamp::now(),
    );
    assert_eq!(outcome.decision, GateDecision::Allow);
    assert_eq!(outcome.audit.origin, ActionOrigin::Kernel);
}

#[test]
fn kernel_origin_call_outside_trusted_set_is_denied() {
    let grant = grant_with(&[], &[], &[]);
    let req = request_for("email.send");
    let ctx = MockContext::default();
    let outcome = gate(
        &grant,
        &req,
        ActionOrigin::Kernel,
        &ctx,
        &test_catalog(),
        &NoEgress,
        Timestamp::now(),
    );
    assert_eq!(
        outcome.decision,
        GateDecision::Deny {
            reason: DenialReason::KernelOriginNotTrusted
        }
    );
}

#[test]
fn kernel_origin_unknown_action_is_unknown_not_trusted() {
    let grant = grant_with(&[], &[], &[]);
    let req = request_for("totally.unknown.action");
    let ctx = MockContext::default();
    let outcome = gate(
        &grant,
        &req,
        ActionOrigin::Kernel,
        &ctx,
        &test_catalog(),
        &NoEgress,
        Timestamp::now(),
    );
    assert_eq!(
        outcome.decision,
        GateDecision::Deny {
            reason: DenialReason::UnknownAction
        }
    );
}

#[test]
fn token_requiring_action_denied_without_token() {
    let grant = grant_with_token(&["email.read_thread:selected_no_attachments"], Ulid::new());
    let req = request_for("email.read_thread:selected_no_attachments");
    let ctx = MockContext::default();
    let outcome = gate(
        &grant,
        &req,
        ActionOrigin::Shell,
        &ctx,
        &test_catalog(),
        &NoEgress,
        Timestamp::now(),
    );
    assert_eq!(
        outcome.decision,
        GateDecision::Deny {
            reason: DenialReason::SelectionTokenInvalid
        }
    );
}

#[test]
fn token_requiring_action_denied_when_token_missing() {
    let token_id = Ulid::new();
    let grant = grant_with_token(&["email.read_thread:selected_no_attachments"], token_id);
    let req = request_for_token("email.read_thread:selected_no_attachments", token_id);
    let ctx = MockContext::default();
    let outcome = gate(
        &grant,
        &req,
        ActionOrigin::Shell,
        &ctx,
        &test_catalog(),
        &NoEgress,
        Timestamp::now(),
    );
    assert_eq!(
        outcome.decision,
        GateDecision::Deny {
            reason: DenialReason::SelectionTokenInvalid
        }
    );
}

#[test]
fn token_requiring_action_denied_for_foreign_grant() {
    let token_id = Ulid::new();
    let other_id = Ulid::new();
    let mut grant = grant_with(&["email.read_thread:selected_no_attachments"], &[], &[]);
    grant.selection_tokens.push(other_id);
    // selection_tokens are MAC-bound; re-seal after binding the foreign token.
    grant.seal_root(b"openspine-test-grant-hmac-key-v1");
    let req = request_for_token("email.read_thread:selected_no_attachments", token_id);
    let mut ctx = MockContext::default();
    ctx.tokens.insert(
        token_id,
        make_token(
            token_id,
            Timestamp::now() + std::time::Duration::from_secs(600),
        ),
    );
    let outcome = gate(
        &grant,
        &req,
        ActionOrigin::Shell,
        &ctx,
        &test_catalog(),
        &NoEgress,
        Timestamp::now(),
    );
    assert_eq!(
        outcome.decision,
        GateDecision::Deny {
            reason: DenialReason::SelectionTokenInvalid
        }
    );
}

#[test]
fn token_requiring_action_denied_when_expired() {
    let token_id = Ulid::new();
    let grant = grant_with_token(&["email.read_thread:selected_no_attachments"], token_id);
    let req = request_for_token("email.read_thread:selected_no_attachments", token_id);
    let mut ctx = MockContext::default();
    ctx.tokens.insert(
        token_id,
        make_token(
            token_id,
            Timestamp::now() - std::time::Duration::from_secs(10),
        ),
    );
    let outcome = gate(
        &grant,
        &req,
        ActionOrigin::Shell,
        &ctx,
        &test_catalog(),
        &NoEgress,
        Timestamp::now(),
    );
    assert_eq!(
        outcome.decision,
        GateDecision::Deny {
            reason: DenialReason::SelectionTokenInvalid
        }
    );
}

#[test]
fn token_requiring_action_denied_when_wrong_type() {
    let token_id = Ulid::new();
    let grant = grant_with_token(&["email.read_thread:selected_no_attachments"], token_id);
    let req = request_for_token("email.read_thread:selected_no_attachments", token_id);
    let mut ctx = MockContext::default();
    let mut wrong_type_token = make_token(
        token_id,
        Timestamp::now() + std::time::Duration::from_secs(600),
    );
    wrong_type_token.token_type = SelectionTokenType::new("some_future_type");
    ctx.tokens.insert(token_id, wrong_type_token);
    let outcome = gate(
        &grant,
        &req,
        ActionOrigin::Shell,
        &ctx,
        &test_catalog(),
        &NoEgress,
        Timestamp::now(),
    );
    assert_eq!(
        outcome.decision,
        GateDecision::Deny {
            reason: DenialReason::SelectionTokenInvalid
        }
    );
}

#[test]
fn token_requiring_action_allowed_with_valid_token() {
    let token_id = Ulid::new();
    let grant = grant_with_token(&["email.read_thread:selected_no_attachments"], token_id);
    let req = request_for_token("email.read_thread:selected_no_attachments", token_id);
    let mut ctx = MockContext::default();
    ctx.tokens.insert(
        token_id,
        make_token(
            token_id,
            Timestamp::now() + std::time::Duration::from_secs(600),
        ),
    );
    let outcome = gate(
        &grant,
        &req,
        ActionOrigin::Shell,
        &ctx,
        &test_catalog(),
        &NoEgress,
        Timestamp::now(),
    );
    assert_eq!(outcome.decision, GateDecision::Allow);
}

pub(crate) fn test_catalog() -> ActionCatalog {
    let ids = [
        "openspine.status.read",
        "workflow.invoke:approved",
        "artifact.propose",
        "setup.workflow.start",
        "memory.read:owner_preferences_limited",
        "model.generate:approved_provider",
        "lyra.ui.preview",
        "telegram.reply:owner_channel",
        "connector.enable",
        "route.activate",
        "capability_pack.change",
        "workflow.activate",
        "policy.change_proposal",
        "email.read_inbox",
        "email.read_thread:unselected",
        "email.send",
        "email.read_attachment",
        "network.raw_egress",
        "vault.secret_read",
        "policy.modify_direct",
        "filesystem.host_read",
        "filesystem.host_write",
        "coolify.deploy",
        "coolify.rollback",
        "coolify.secret_modify",
        "email.read_thread:selected_no_attachments",
        "memory.read:writing_preferences_scoped",
        "artifact.write:task_scratch",
        "email.create_draft",
        "artifact.activate",
        "coolify.delete_resource",
        "owner.notify",
    ];
    ActionCatalog::new(ids.iter().map(|s| ActionId::new(*s)))
        .with_kernel_origin([ActionId::new("owner.notify")])
        .with_token_requiring([(
            ActionId::new("email.read_thread:selected_no_attachments"),
            SelectionTokenType::email_thread_selection(),
        )])
}

#[test]
fn gate_denies_catalog_unknown_id_with_unknown_action_reason() {
    let grant = grant_with(&["openspine.status.read"], &[], &[]);
    let req = request_for("totally.unknown.action");
    let ctx = MockContext::default();
    let outcome = gate(
        &grant,
        &req,
        ActionOrigin::Shell,
        &ctx,
        &test_catalog(),
        &NoEgress,
        Timestamp::now(),
    );
    assert!(
        matches!(
            outcome.decision,
            GateDecision::Deny {
                reason: DenialReason::UnknownAction
            }
        ),
        "expected UnknownAction denial, got {:?}",
        outcome.decision
    );
    assert_eq!(outcome.audit.action, req.action);
}

#[test]
fn gate_keeps_not_granted_for_known_ungranted_id() {
    let grant = grant_with(&["email.read_inbox"], &[], &[]);
    let req = request_for("email.send");
    let ctx = MockContext::default();
    let outcome = gate(
        &grant,
        &req,
        ActionOrigin::Shell,
        &ctx,
        &test_catalog(),
        &NoEgress,
        Timestamp::now(),
    );
    assert!(
        matches!(
            outcome.decision,
            GateDecision::Deny {
                reason: DenialReason::NotGranted
            }
        ),
        "expected NotGranted for a known-but-ungranted id, got {:?}",
        outcome.decision
    );
}

#[test]
fn gate_denies_stale_granted_but_catalog_unknown_id() {
    let grant = grant_with(&["totally.unknown.action"], &[], &[]);
    let req = request_for("totally.unknown.action");
    let ctx = MockContext::default();
    let outcome = gate(
        &grant,
        &req,
        ActionOrigin::Shell,
        &ctx,
        &test_catalog(),
        &NoEgress,
        Timestamp::now(),
    );
    assert!(
        matches!(
            outcome.decision,
            GateDecision::Deny {
                reason: DenialReason::UnknownAction
            }
        ),
        "expected UnknownAction for a stale-granted out-of-catalog id, got {:?}",
        outcome.decision
    );
}
#[test]
fn expired_invalid_mac_is_caveat_widening_not_grant_expired() {
    // D-004: chain integrity is classified before expiry. An expired grant
    // with a broken MAC must surface CaveatWidening, not GrantExpired.
    let mut grant = grant_with(&["openspine.status.read"], &[], &[]);
    grant.expires_at = Timestamp::now() - std::time::Duration::from_secs(60);
    grant.caveat_mac = "00".repeat(32);
    let outcome = gate(
        &grant,
        &request_for("openspine.status.read"),
        ActionOrigin::Shell,
        &MockContext::default(),
        &test_catalog(),
        &NoEgress,
        Timestamp::now(),
    );
    assert_eq!(
        outcome.decision,
        GateDecision::Deny {
            reason: DenialReason::CaveatWidening
        }
    );
}
