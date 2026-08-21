//! Root-owner counterparty crypto-erasure origination handler (#172).
//!
//! Counterparty crypto-erasure (AD-140) is production-wired and unconditionally
//! reconciled at startup, but until now the erasure set could only be populated
//! by an accepted external terminal ledger on restore — there was no *local*
//! owner origination surface. This handler is that surface: a non-delegable
//! root-owner gate-mediated action, modeled on `openspine.overlay.export` /
//! `restore`, that drives [`crate::counterparty_erasure::erase_counterparty`].
//!
//! Ordering is the primitive's: the signed terminal-ledger entry is durable
//! FIRST, then the local transactional erasure sweep (learned-artifact
//! invalidation, audit row, durable erased-scope marker) and the irreversible
//! key deletion. This handler only authorizes the owner and validates input.

use std::str::FromStr;

use serde::Deserialize;
use serde_json::{json, Value};
use ulid::Ulid;

use openspine_schemas::action::ActionId;
use openspine_schemas::grant::TaskGrant;
use openspine_schemas::owner_surface::OwnerSurfaceRef;

use super::actions::DispatchError;
use super::handler_registry::HandlerFuture;
use super::root_owner_grant::require_root_owner_grant;
use crate::counterparty_erasure::{erase_counterparty, CounterpartyEraseError};
use crate::pipeline::AppState;

const ERASE_ACTION: &str = "openspine.counterparty.erase";
const AUTH_LABEL: &str = "counterparty erasure";

/// Strict one-field payload: the ULID of the counterparty to erase.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ErasePayload {
    counterparty_id: String,
}

/// Parse and validate the payload into a counterparty ULID. Every decode
/// failure folds to a `BadRequest` with a static, non-leaking message.
fn parse_payload(payload: Option<&Value>) -> Result<Ulid, DispatchError> {
    let payload = payload
        .ok_or_else(|| DispatchError::BadRequest(format!("{ERASE_ACTION} requires a payload")))?;
    let req: ErasePayload = serde_json::from_value(payload.clone()).map_err(|_| {
        DispatchError::BadRequest(format!(
            "{ERASE_ACTION} payload must be exactly {shape}",
            shape = r#"{"counterparty_id": string}"#
        ))
    })?;
    Ulid::from_str(&req.counterparty_id)
        .map_err(|_| DispatchError::BadRequest("counterparty_id is not a valid id".to_string()))
}

/// A post-authorization erasure failure is an infrastructure fault, not caller
/// input: the two caller-facing rejections (invalid ULID, `SYSTEM_SCOPE`) are
/// handled before the primitive runs, so everything the primitive can return is
/// a store/artifact/ledger fault routed onto the kernel error lane.
fn map_erase_error(err: CounterpartyEraseError) -> DispatchError {
    DispatchError::Resource(anyhow::Error::new(err))
}

pub(crate) fn handle_erase_counterparty<'a>(
    state: &'a AppState,
    grant: &'a TaskGrant,
    action: &'a ActionId,
    _owner_surface: &OwnerSurfaceRef,
    payload: Option<&'a Value>,
) -> HandlerFuture<'a> {
    Box::pin(async move {
        if action.as_str() != ERASE_ACTION {
            return Err(DispatchError::BadRequest(
                "handler registered for openspine.counterparty.erase only".to_string(),
            ));
        }
        require_root_owner_grant(state, grant, action, AUTH_LABEL)?;
        let counterparty_id = parse_payload(payload)?;

        // Defense in depth: the primitive also rejects SYSTEM_SCOPE, but doing
        // it here keeps the reserved-scope refusal a clean 400 instead of a
        // 500 on the kernel error lane.
        if counterparty_id == crate::counterparty_keys::SYSTEM_SCOPE {
            return Err(DispatchError::BadRequest(
                "SYSTEM_SCOPE is reserved and cannot be erased".to_string(),
            ));
        }

        let report = erase_counterparty(
            &state.store,
            &state.artifacts,
            state.overlay_operations.as_ref(),
            counterparty_id,
        )
        .map_err(map_erase_error)?;

        // Deliberately omit `invalidated_identities`: returning the exact
        // learned-artifact identities would leak provenance detail into the
        // reply. Only the non-sensitive counts and the ledger sequence surface.
        Ok(json!({
            "counterparty_id": counterparty_id.to_string(),
            "derived_artifacts_invalidated": report.derived_artifacts_invalidated,
            "key_deleted": report.key_deleted,
            "ledger_sequence": report.ledger_sequence,
        }))
    })
}

#[cfg(test)]
#[path = "erase_counterparty_tests.rs"]
mod tests;
