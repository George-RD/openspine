use crate::artifact_store::ArtifactStoreError;
use crate::failure_surfacing::FailureClass;
use crate::pipeline::AppState;
use crate::store::StoreError;
use anyhow::Error;
use openspine_schemas::action::{ActionId, ActionRequest};
use openspine_schemas::digest::{canonical_json, digest_of};
use openspine_schemas::event::{TargetRef, TargetRefKind};
use openspine_schemas::grant::TaskGrant;
use serde_json::json;
use ulid::Ulid;

use super::actions::{DispatchError, PreviewPayload};
use super::connector_breaker::call_with_connector;

#[derive(Debug, thiserror::Error)]
pub(super) enum ProposalError {
    #[error("no Gmail connector configured")]
    NoGmailConnector,
    #[error("grant has no selection token")]
    NoSelectionToken,
    #[error("selection token lookup failed: {0}")]
    SelectionTokenLookup(#[source] StoreError),
    #[error("selection token not found")]
    SelectionTokenNotFound,
    #[error("Gmail outcome counter persistence failed: {0}")]
    GmailOutcomeRecord(#[source] Error),
    #[error("Gmail connector call failed (fetch/admission/timeout): {0}")]
    GmailCall(#[source] Error),
    #[error("Gmail connector unavailable (breaker open): {0}")]
    GmailUnavailable(#[source] Error),
    #[error("no non-owner recipient found")]
    NoNonOwnerRecipient,
    #[error("artifact budget check failed: {0}")]
    ArtifactBudgetCheck(#[source] StoreError),
    #[error("artifact budget exhausted")]
    ArtifactBudgetExhausted,
    #[error("artifact payload persistence failed: {0}")]
    ArtifactStore(#[source] ArtifactStoreError),
    #[error("daily spend cap exceeded")]
    SpendGuard(#[source] anyhow::Error),
    #[error("action request persistence failed: {0}")]
    ActionRequestPersist(#[source] StoreError),
}

impl ProposalError {
    /// Whether this error is a durable, fatal `Resource` failure that must
    /// be surfaced as a truthful dispatch error (never misreported as a
    /// successful preview), versus a recoverable `Connector`-class failure
    /// where the preview is still shown without an approval button.
    pub(super) fn failure_class(&self) -> FailureClass {
        match self {
            Self::SelectionTokenLookup(_)
            | Self::GmailOutcomeRecord(_)
            | Self::ArtifactBudgetCheck(_)
            | Self::ArtifactBudgetExhausted
            | Self::ArtifactStore(_)
            | Self::SpendGuard(_)
            | Self::ActionRequestPersist(_) => FailureClass::Resource,
            Self::NoGmailConnector
            | Self::NoSelectionToken
            | Self::SelectionTokenNotFound
            | Self::GmailCall(_)
            | Self::GmailUnavailable(_)
            | Self::NoNonOwnerRecipient => FailureClass::Connector,
        }
    }
}

pub(super) async fn propose_draft_creation(
    state: &AppState,
    grant: &TaskGrant,
    action: &ActionId,
    preview: &PreviewPayload,
) -> Result<Ulid, ProposalError> {
    let gmail = state
        .connectors
        .gmail()
        .ok_or(ProposalError::NoGmailConnector)?;
    let token_id = grant
        .selection_tokens
        .first()
        .copied()
        .ok_or(ProposalError::NoSelectionToken)?;
    let token = state
        .store
        .find_selection_token(token_id)
        .map_err(ProposalError::SelectionTokenLookup)?
        .ok_or(ProposalError::SelectionTokenNotFound)?;
    crate::spend::guard_connector_for(state, grant)
        .await
        .map_err(ProposalError::SpendGuard)?;
    // AD-103/AD-141: admit + bound-timeout the Gmail fetch at the call site;
    // the helper records breaker health and the D-069 counter. A genuinely
    // Open breaker surfaces as `GmailUnavailable` so the caller can propagate
    // the distinct `connector_unavailable` outcome without re-batching.
    let thread = call_with_connector(
        state,
        "gmail",
        action,
        grant,
        gmail.fetch_thread(&token.target_id),
    )
    .await
    .map_err(|e| match e {
        DispatchError::ConnectorUnavailable(c) => ProposalError::GmailUnavailable(c),
        DispatchError::Connector(c) | DispatchError::DeliveryUnknown(c) => {
            ProposalError::GmailCall(c)
        }
        DispatchError::BadRequest(msg) => ProposalError::GmailCall(anyhow::anyhow!(msg)),
        DispatchError::Resource(c) => ProposalError::GmailOutcomeRecord(c),
    })?;
    let target = crate::gmail::newest_non_owner_recipient(&thread, gmail.mailbox_address())
        .ok_or(ProposalError::NoNonOwnerRecipient)?;

    if !state
        .store
        .try_count_artifact_put(grant.id, grant.limits.max_artifacts)
        .map_err(ProposalError::ArtifactBudgetCheck)?
    {
        return Err(ProposalError::ArtifactBudgetExhausted);
    }
    let payload_bytes = canonical_json(&json!({
        "subject": preview.subject,
        "body": preview.body
    }));
    let payload_ref = state
        .artifacts
        .put(payload_bytes.as_bytes())
        .map_err(ProposalError::ArtifactStore)?;
    let target_digest = digest_of(&json!({
        "thread_id": token.target_id,
        "connector": "gmail_primary",
        "account_role": "owner_mailbox",
        "recipients": [target.recipient],
    }));

    let request = ActionRequest {
        id: Ulid::new(),
        task_grant_id: grant.id,
        action: ActionId::new("email.create_draft"),
        target_ref: Some(TargetRef {
            kind: TargetRefKind::EmailThread,
            id: Some(token.target_id.clone()),
        }),
        payload_ref: Some(payload_ref),
        target_digest: Some(target_digest),
        selection_token_id: None,
        params: std::collections::BTreeMap::new(),
        requested_at: jiff::Timestamp::now(),
        schema_version: 1,
    };
    state
        .store
        .insert_action_request(&request)
        .map_err(ProposalError::ActionRequestPersist)?;
    Ok(request.id)
}
