//! Storage for the three tables that exist purely to make `openspine_gate`'s
//! `GateContext` lookups possible: approvals, selection tokens, and pending
//! action requests (D-040/D-043/D-044). Split out of `store/mod.rs` to keep
//! that file under the 500-line gate — these tables are conceptually one
//! group (everything the approval/selection flow touches) separate from the
//! task-grant/audit-log core.

use openspine_schemas::action::ActionRequest;
use openspine_schemas::approval::ApprovalRecord;
use openspine_schemas::selection::SelectionToken;
use rusqlite::{params, OptionalExtension};
use ulid::Ulid;

use super::{Store, StoreError};

impl Store {
    // ---- approvals ----------------------------------------------------

    /// Called by `lyra.ui.preview`'s dispatch (Step 5/D-043) when it
    /// proposes `email.create_draft` for the exact reviewed draft, and by
    /// the `callback_query` approval handler (D-044) once the owner taps
    /// "Approve".
    pub fn insert_approval(&self, approval: &ApprovalRecord) -> Result<(), StoreError> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO approvals (id, action_request_id, approval_json) VALUES (?1, ?2, ?3)",
            params![
                approval.id.to_string(),
                approval.action_request_id.to_string(),
                serde_json::to_string(approval)?,
            ],
        )?;
        Ok(())
    }

    /// Most recent approval decision recorded against `action_request_id`,
    /// if any (backs `openspine_gate::GateContext::approval_for_request`).
    pub fn find_approval_for_request(
        &self,
        action_request_id: Ulid,
    ) -> Result<Option<ApprovalRecord>, StoreError> {
        let conn = self.conn.lock();
        let json: Option<String> = conn
            .query_row(
                "SELECT approval_json FROM approvals WHERE action_request_id = ?1 ORDER BY rowid DESC LIMIT 1",
                params![action_request_id.to_string()],
                |row| row.get(0),
            )
            .optional()?;
        Ok(json.map(|j| serde_json::from_str(&j)).transpose()?)
    }

    // ---- selection tokens ----------------------------------------------

    /// Called by `pipeline::handle_thread_selection` (Step 5) when it
    /// mints a new thread-selection token.
    pub fn insert_selection_token(&self, token: &SelectionToken) -> Result<(), StoreError> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO selection_tokens (id, used, token_json) VALUES (?1, 0, ?2)",
            params![token.id.to_string(), serde_json::to_string(token)?],
        )?;
        Ok(())
    }

    /// Backs `openspine_gate::GateContext::find_selection_token` (Step 5).
    pub fn find_selection_token(&self, id: Ulid) -> Result<Option<SelectionToken>, StoreError> {
        let conn = self.conn.lock();
        let json: Option<String> = conn
            .query_row(
                "SELECT token_json FROM selection_tokens WHERE id = ?1",
                params![id.to_string()],
                |row| row.get(0),
            )
            .optional()?;
        Ok(json.map(|j| serde_json::from_str(&j)).transpose()?)
    }

    /// Atomically consume a single-use selection token (PRD §15):
    /// `UPDATE ... WHERE used = 0` in one locked statement, returning
    /// whether *this call* was the one that flipped it. Checking
    /// (`SELECT used`) and marking (`UPDATE ... SET used = 1`) as two
    /// separate calls would be a TOCTOU race — the dispatch path awaits a
    /// Gmail HTTP call between "is it used" and "mark it used", so two
    /// concurrent requests for the same token could both observe `unused`
    /// before either marks it. Consumed *before* the Gmail call, same
    /// at-most-once philosophy as the Telegram `update_id` offset
    /// (persisted before handling, not after): a token whose consumption
    /// wins but whose subsequent Gmail fetch fails is burned, not
    /// refunded — the owner just runs `/draft` again for a fresh one.
    pub fn try_consume_selection_token(&self, id: Ulid) -> Result<bool, StoreError> {
        let conn = self.conn.lock();
        let rows = conn.execute(
            "UPDATE selection_tokens SET used = 1 WHERE id = ?1 AND used = 0",
            params![id.to_string()],
        )?;
        Ok(rows > 0)
    }

    // ---- pending action requests (D-040) --------------------------------

    /// Persist an [`ActionRequest`] proposed for approval (D-043's
    /// `lyra.ui.preview` extension) so a later `callback_query` approval
    /// can be correlated back to the *exact* request it decides
    /// (`openspine_gate::GateContext::approval_for_request` requires the
    /// same `id` to resolve both times). No update path — a request is
    /// immutable once proposed (D-011: mutating it after the fact is
    /// exactly the digest-spoofing attack this whole mechanism prevents).
    pub fn insert_action_request(&self, request: &ActionRequest) -> Result<(), StoreError> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO action_requests (id, request_json, used) VALUES (?1, ?2, 0)",
            params![request.id.to_string(), serde_json::to_string(request)?],
        )?;
        Ok(())
    }

    /// Look up a previously-proposed [`ActionRequest`] by id (D-044's
    /// `callback_query` handler, before re-running `gate()` against it).
    pub fn find_action_request(&self, id: Ulid) -> Result<Option<ActionRequest>, StoreError> {
        let conn = self.conn.lock();
        let json: Option<String> = conn
            .query_row(
                "SELECT request_json FROM action_requests WHERE id = ?1",
                params![id.to_string()],
                |row| row.get(0),
            )
            .optional()?;
        Ok(json.map(|j| serde_json::from_str(&j)).transpose()?)
    }

    /// Atomically consume a pending [`ActionRequest`] before dispatching
    /// its approval (D-044): `UPDATE ... WHERE used = 0` in one locked
    /// statement, mirroring [`Self::try_consume_selection_token`]'s
    /// at-most-once contract. The owner's "Approve" button stays live on
    /// the Telegram message indefinitely (Telegram never removes inline
    /// keyboards on its own, and `answerCallbackQuery` doesn't disable
    /// them either) — without this, a second tap, or Telegram redelivering
    /// the same `callback_query` update, would mint a second
    /// `ApprovalRecord` and create a second Gmail draft for the same
    /// request. Consumed before the Gmail draft call, same reasoning as
    /// selection tokens: a request whose consumption wins but whose
    /// subsequent Gmail call fails is burned, not refunded.
    pub fn try_consume_action_request(&self, id: Ulid) -> Result<bool, StoreError> {
        let conn = self.conn.lock();
        let rows = conn.execute(
            "UPDATE action_requests SET used = 1 WHERE id = ?1 AND used = 0",
            params![id.to_string()],
        )?;
        Ok(rows > 0)
    }
}
