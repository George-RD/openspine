//! Macaroons-simple grant chain (AD-101): parent-derived HMAC hops, offline
//! verification, no parent DB lookup.
//!
//! ```text
//! tip₀ = HMAC(key, canonical(root_authority))   // immutable lists/limits/expiry + bindings
//! for each ChainStep in order:
//!   tip = fold HMAC(tip, caveat) over step.added_caveats
//!   tip = HMAC(tip, canonical(step bind: grant_id, parent, mode, selection_tokens))
//! caveat_mac = hex(final tip)
//! ```
//! A child holder of the parent tip appends one step (new caveats + child bind)
use hmac_sha256::HMAC;
use serde::{Deserialize, Serialize};
use ulid::Ulid;

use crate::action::ActionId;
use crate::digest::canonical_json;
use crate::egress::EgressClass;
use crate::grant::{GrantLimits, GrantMode, TaskGrant};

/// One ordered Macaroons-simple caveat (AD-101 / AD-036).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Caveat {
    ActionAllowlist { actions: Vec<ActionId> },
    BoundParameter { name: String, value: String },
    ExpiresBefore { at: jiff::Timestamp },
    ModelTier { max_tier: String },
    OutputChannelAllowlist { channels: Vec<String> },
}

/// One hop in the parent-derived chain. Roots use a single step with
/// `parent_grant_id = None`. Children append a step whose parent is the
/// previous hop's `grant_id`. `selection_tokens` is MAC-covered per hop so
/// a valid tip cannot authorize extra tokens (gate.rs trusts this list).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChainStep {
    pub grant_id: Ulid,
    #[serde(default)]
    pub parent_grant_id: Option<Ulid>,
    pub mode: GrantMode,
    #[serde(default)]
    pub selection_tokens: Vec<Ulid>,
    #[serde(default)]
    pub added_caveats: Vec<Caveat>,
}

/// Immutable root authority + routing/identity bindings authenticated as the
/// chain base commitment. Every field gate or routing trusts that is fixed
/// for the chain lifetime belongs here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootAuthority {
    pub root_grant_id: Ulid,
    pub expires_at: jiff::Timestamp,
    pub allowed_actions: Vec<ActionId>,
    pub approval_required_actions: Vec<ActionId>,
    pub denied_actions: Vec<ActionId>,
    /// AD-060: MAC-bound egress classes. A shell cannot widen these without
    /// breaking the tip.
    pub allowed_egress_classes: Vec<EgressClass>,
    pub output_channels: Vec<String>,
    pub limits: GrantLimits,
    pub user: String,
    pub purpose: String,
    pub event_id: Ulid,
    pub route_id: String,
    pub agent_id: String,
    pub workflow_id: String,
    pub capability_pack_id: String,
    /// Kernel-owned conversation binding, authenticated with root authority.
    /// Dormant until a thread-capable channel ships (AD-148).
    pub thread_id: Option<String>,
}

impl RootAuthority {
    pub fn from_grant(grant: &TaskGrant) -> Self {
        Self {
            root_grant_id: grant.root_grant_id,
            expires_at: grant.expires_at,
            allowed_actions: grant.allowed_actions.clone(),
            approval_required_actions: grant.approval_required_actions.clone(),
            denied_actions: grant.denied_actions.clone(),
            allowed_egress_classes: grant.allowed_egress_classes.clone(),
            output_channels: grant.output_channels.clone(),
            limits: grant.limits,
            user: grant.user.clone(),
            purpose: grant.purpose.clone(),
            event_id: grant.event_id,
            route_id: grant.route_id.clone(),
            agent_id: grant.agent_id.clone(),
            workflow_id: grant.workflow_id.clone(),
            capability_pack_id: grant.capability_pack_id.clone(),
            thread_id: grant.thread_id.clone(),
        }
    }

    fn canonical_bytes(&self) -> Vec<u8> {
        let mut allowed: Vec<String> = self
            .allowed_actions
            .iter()
            .map(|a| a.as_str().to_string())
            .collect();
        allowed.sort();
        let mut approval: Vec<String> = self
            .approval_required_actions
            .iter()
            .map(|a| a.as_str().to_string())
            .collect();
        approval.sort();
        let mut denied: Vec<String> = self
            .denied_actions
            .iter()
            .map(|a| a.as_str().to_string())
            .collect();
        denied.sort();
        let mut egress: Vec<String> = self
            .allowed_egress_classes
            .iter()
            .map(|c| c.as_str().to_string())
            .collect();
        egress.sort();
        let mut channels = self.output_channels.clone();
        channels.sort();
        let mut payload = serde_json::json!({
            "root_grant_id": self.root_grant_id.to_string(),
            "expires_at": self.expires_at.to_string(),
            "allowed_actions": allowed,
            "approval_required_actions": approval,
            "denied_actions": denied,
            "output_channels": channels,
            "limits": {
                "max_model_calls": self.limits.max_model_calls,
                "max_artifacts": self.limits.max_artifacts,
                "max_runtime_seconds": self.limits.max_runtime_seconds,
            },
            "user": self.user,
            "purpose": self.purpose,
            "event_id": self.event_id.to_string(),
            "route_id": self.route_id,
            "agent_id": self.agent_id,
            "workflow_id": self.workflow_id,
            "capability_pack_id": self.capability_pack_id,
        });
        // Backward compatibility: a grant minted before AD-060 had no
        // egress-class concept, so its MAC was computed over a payload
        // that never carried this key. Omitting the key entirely when the
        // list is empty keeps the canonical bytes — and therefore the
        // MAC — byte-identical to the pre-change shape for every grant
        // that predates this field (default/empty). A grant that DOES
        // carry rated classes still gets the key, and any mutation of a
        // non-empty list still changes the payload and invalidates the
        // MAC exactly as any other authority field does.
        if !egress.is_empty() {
            payload["allowed_egress_classes"] = serde_json::json!(egress);
        }
        // Preserve the pre-thread binding canonical form for legacy grants.
        // Adding Some(thread_id) still changes the committed bytes and fails
        // verification unless the authority is resealed.
        if let Some(thread_id) = &self.thread_id {
            payload["thread_id"] = serde_json::Value::String(thread_id.clone());
        }
        canonical_json(&payload).into_bytes()
    }
}

fn caveat_bytes(caveat: &Caveat) -> Vec<u8> {
    let v = match caveat {
        Caveat::ActionAllowlist { actions } => {
            let mut a: Vec<String> = actions.iter().map(|x| x.as_str().to_string()).collect();
            a.sort();
            serde_json::json!({"kind":"action_allowlist","actions":a})
        }
        Caveat::BoundParameter { name, value } => {
            serde_json::json!({"kind":"bound_parameter","name":name,"value":value})
        }
        Caveat::ExpiresBefore { at } => {
            serde_json::json!({"kind":"expires_before","at":at.to_string()})
        }
        Caveat::ModelTier { max_tier } => {
            serde_json::json!({"kind":"model_tier","max_tier":max_tier})
        }
        Caveat::OutputChannelAllowlist { channels } => {
            let mut c = channels.clone();
            c.sort();
            serde_json::json!({"kind":"output_channel_allowlist","channels":c})
        }
    };
    canonical_json(&v).into_bytes()
}

fn step_bind_bytes(step: &ChainStep) -> Vec<u8> {
    let mut tokens: Vec<String> = step
        .selection_tokens
        .iter()
        .map(|t| t.to_string())
        .collect();
    tokens.sort();
    let payload = serde_json::json!({
        "grant_id": step.grant_id.to_string(),
        "parent_grant_id": step.parent_grant_id.map(|p| p.to_string()),
        "mode": match step.mode {
            GrantMode::Live => "live",
            GrantMode::Shadow => "shadow",
        },
        "selection_tokens": tokens,
    });
    canonical_json(&payload).into_bytes()
}

fn hmac_sha256(key: &[u8], msg: &[u8]) -> [u8; 32] {
    HMAC::mac(msg, key)
}

fn hex_encode(bytes: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(64);
    for &b in bytes {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0xf) as usize] as char);
    }
    s
}

fn hex_decode_32(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for i in 0..32 {
        let hi = u8::from_str_radix(&s[i * 2..i * 2 + 1], 16).ok()?;
        let lo = u8::from_str_radix(&s[i * 2 + 1..i * 2 + 2], 16).ok()?;
        out[i] = (hi << 4) | lo;
    }
    Some(out)
}

fn ct_eq_hex(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.as_bytes()
        .iter()
        .zip(b.as_bytes())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

pub fn compute_tip(key: &[u8], root: &RootAuthority, chain: &[ChainStep]) -> Option<[u8; 32]> {
    if chain.is_empty() {
        return None;
    }
    let mut tip = hmac_sha256(key, &root.canonical_bytes());
    for step in chain {
        for caveat in &step.added_caveats {
            tip = hmac_sha256(&tip, &caveat_bytes(caveat));
        }
        tip = hmac_sha256(&tip, &step_bind_bytes(step));
    }
    Some(tip)
}

pub fn compute_mac_hex(key: &[u8], root: &RootAuthority, chain: &[ChainStep]) -> String {
    match compute_tip(key, root, chain) {
        Some(tip) => hex_encode(&tip),
        None => String::new(),
    }
}

pub fn chain_structurally_valid(grant: &TaskGrant) -> bool {
    if grant.root_grant_id.is_nil() || grant.chain.is_empty() {
        return false;
    }
    let first = &grant.chain[0];
    if first.grant_id != grant.root_grant_id || first.parent_grant_id.is_some() {
        return false;
    }
    let mut effective_tokens = std::collections::BTreeSet::new();
    let mut shadow_seen = false;
    for (i, step) in grant.chain.iter().enumerate() {
        if i > 0 {
            let prev = &grant.chain[i - 1];
            if step.parent_grant_id != Some(prev.grant_id) {
                return false;
            }
            let next: std::collections::BTreeSet<_> =
                step.selection_tokens.iter().copied().collect();
            if !next.is_subset(&effective_tokens) {
                return false;
            }
            effective_tokens = next;
        } else {
            effective_tokens.extend(step.selection_tokens.iter().copied());
        }
        if shadow_seen && step.mode == GrantMode::Live {
            return false;
        }
        shadow_seen |= step.mode == GrantMode::Shadow;
    }
    let last = grant.chain.last().expect("non-empty");
    if last.grant_id != grant.id
        || last.parent_grant_id != grant.parent_grant_id
        || last.mode != grant.mode
    {
        return false;
    }
    let mut a = last.selection_tokens.clone();
    let mut b = grant.selection_tokens.clone();
    a.sort();
    b.sort();
    a == b
}

pub fn verify_mac(key: &[u8], grant: &TaskGrant) -> bool {
    if grant.caveat_mac.is_empty() || !chain_structurally_valid(grant) {
        return false;
    }
    let root = RootAuthority::from_grant(grant);
    let expected = compute_mac_hex(key, &root, &grant.chain);
    ct_eq_hex(&expected, &grant.caveat_mac)
}

/// Seal a root after all authority fields (including token bindings) are final.
pub fn seal_root(grant: &mut TaskGrant, key: &[u8]) {
    if grant.root_grant_id.is_nil() {
        grant.root_grant_id = grant.id;
    }
    grant.parent_grant_id = None;
    grant.chain = vec![ChainStep {
        grant_id: grant.id,
        parent_grant_id: None,
        mode: grant.mode,
        selection_tokens: grant.selection_tokens.clone(),
        added_caveats: vec![],
    }];
    let root = RootAuthority::from_grant(grant);
    grant.caveat_mac = compute_mac_hex(key, &root, &grant.chain);
}

pub fn seal_child_from_parent_tip(parent_tip_hex: &str, child_step: &ChainStep) -> Option<String> {
    let mut tip = hex_decode_32(parent_tip_hex)?;
    for caveat in &child_step.added_caveats {
        tip = hmac_sha256(&tip, &caveat_bytes(caveat));
    }
    tip = hmac_sha256(&tip, &step_bind_bytes(child_step));
    Some(hex_encode(&tip))
}

/// Reject conflicting AD-036 bindings in one chain. A later caveat may add a
/// name but may never change its already-bound value.
pub fn bindings_valid(grant: &TaskGrant) -> bool {
    let mut bindings = std::collections::BTreeMap::<&str, &str>::new();
    for caveat in flattened_caveats(&grant.chain) {
        if let Caveat::BoundParameter { name, value } = caveat {
            if let Some(previous) = bindings.insert(name, value) {
                if previous != value {
                    return false;
                }
            }
        }
    }
    true
}

/// These caveats are authenticated but have no request fields at this gate
/// boundary yet; reject them rather than silently treating them as no-ops.
/// `OutputChannelAllowlist` is supported (see
/// [`effectively_allows_output_channel`]) — the first runtime consumer is
/// `implement-worker-runtime`'s sub-grant minting (AD-101/AD-033).
pub fn has_unsupported_caveats(grant: &TaskGrant) -> bool {
    flattened_caveats(&grant.chain)
        .iter()
        .any(|c| matches!(c, Caveat::ModelTier { .. }))
}

pub fn flattened_caveats(chain: &[ChainStep]) -> Vec<&Caveat> {
    chain.iter().flat_map(|s| s.added_caveats.iter()).collect()
}
/// Effective output-channel membership (mirrors [`effectively_allows`] for
/// actions): `channel` is usable only if it is in the root's
/// `output_channels` AND every `OutputChannelAllowlist` caveat in the chain
/// also names it. A worker sub-grant minted with an empty-channels caveat
/// therefore has zero effective output channels regardless of what its
/// ancestor root carried — the caveat can only narrow, never widen (D-007 /
/// AD-101).
pub fn effectively_allows_output_channel(grant: &TaskGrant, channel: &str) -> bool {
    if !grant.output_channels.iter().any(|c| c == channel) {
        return false;
    }
    for caveat in flattened_caveats(&grant.chain) {
        if let Caveat::OutputChannelAllowlist { channels } = caveat {
            if !channels.iter().any(|c| c == channel) {
                return false;
            }
        }
    }
    true
}

pub fn effectively_allows(grant: &TaskGrant, action: &ActionId) -> bool {
    if !grant.allowed_actions.contains(action) {
        return false;
    }
    for caveat in flattened_caveats(&grant.chain) {
        if let Caveat::ActionAllowlist { actions } = caveat {
            if !actions.contains(action) {
                return false;
            }
        }
    }
    true
}

pub fn effectively_approval_required(grant: &TaskGrant, action: &ActionId) -> bool {
    grant.approval_required_actions.contains(action)
        && flattened_caveats(&grant.chain)
            .iter()
            .all(|c| !matches!(c, Caveat::ActionAllowlist { actions } if !actions.contains(action)))
}

pub fn is_expired(grant: &TaskGrant, now: jiff::Timestamp) -> bool {
    if now >= grant.expires_at {
        return true;
    }
    for caveat in flattened_caveats(&grant.chain) {
        if let Caveat::ExpiresBefore { at } = caveat {
            if now >= *at {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
pub const TEST_GRANT_HMAC_KEY: &[u8] = b"openspine-test-grant-hmac-key-v1";

#[cfg(test)]
mod tests;
