//! Principal-bound, channel-neutral references to authenticated owner surfaces.
//!
//! A surface reference is the single typed handle generic review, decision,
//! pending-action, notification, and receipt code uses to address the owner.
//! It is principal-bound and, for a verified Telegram private chat, carries
//! the connector surface id opaquely — only adapter code (the Telegram
//! connector and notification effect) extracts the raw address. No generic
//! kernel seam may accept a naked `bound_chat_id: i64`.

use serde::{Deserialize, Serialize};
use ulid::Ulid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OwnerSurfaceKind {
    TelegramPrivate,
    LocalTerminal,
    WebOrMobile,
}

/// Kernel-minted proof of which authenticated owner surface submitted input
/// (or is the recipient of an owner-facing notification). Connector rendering
/// identifiers remain adapter-local and are deliberately absent from this
/// contract except for the opaque `surface_id`, which a verified Telegram
/// surface must carry to address the connector; generic code never reads it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnerSurfaceRef {
    kind: OwnerSurfaceKind,
    principal_id: Ulid,
    thread_binding: Option<String>,
    /// Opaque connector surface address, populated only for
    /// [`OwnerSurfaceKind::TelegramPrivate`]. Adapter code resolves it via
    /// [`OwnerSurfaceRef::surface_id`]; generic code does not.
    surface_id: Option<String>,
}

impl OwnerSurfaceRef {
    /// A verified Telegram private owner chat with its connector surface id.
    pub fn verified_telegram(
        principal_id: Ulid,
        surface_id: String,
        thread_binding: Option<String>,
    ) -> Self {
        Self {
            kind: OwnerSurfaceKind::TelegramPrivate,
            principal_id,
            thread_binding,
            surface_id: Some(surface_id),
        }
    }

    /// An authenticated local terminal/device owner session. A terminal has
    /// no Telegram surface id and must never synthesize one.
    pub fn authenticated_terminal(principal_id: Ulid) -> Self {
        Self {
            kind: OwnerSurfaceKind::LocalTerminal,
            principal_id,
            thread_binding: None,
            surface_id: None,
        }
    }

    pub fn kind(&self) -> OwnerSurfaceKind {
        self.kind
    }

    pub fn principal_id(&self) -> Ulid {
        self.principal_id
    }

    pub fn thread_binding(&self) -> Option<&str> {
        self.thread_binding.as_deref()
    }

    /// The opaque connector surface address, when this surface is a verified
    /// private owner chat. Returns `None` for any non-Telegram surface;
    /// generic code must not branch on it — only the Telegram
    /// connector/notification adapter should, and only after confirming
    /// `kind() == TelegramPrivate`.
    pub fn surface_id(&self) -> Option<&str> {
        self.surface_id.as_deref()
    }
}
