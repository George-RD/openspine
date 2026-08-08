use crate::pipeline::{AppState, NotifyOutcome};
use crate::store::failure_surfacing_types::{DetailReceipt, DigestItem};
use openspine_schemas::artifact::ArtifactRef;
use openspine_schemas::digest::Digest;
use openspine_schemas::owner_surface::OwnerSurfaceRef;
use ulid::Ulid;

pub(crate) const TELEGRAM_DIGEST_CAP: usize = 4096;

/// Render a digest page. Every emitted line carries the item's `ref:<ULID>`
/// so the owner can resolve its full protected detail via `/digest <ULID>`.
/// The whole page is kept within Telegram's byte cap; an item whose line
/// would overflow is emitted as a bounded representation that still advertises
/// its resolvable `ref`.
pub(crate) fn render_page(items: &[DigestItem]) -> (String, Vec<Ulid>) {
    const HEADER: &str = "Owner digest";
    let mut page = String::from(HEADER);
    let mut used = HEADER.len();
    let mut delivered = Vec::new();
    for item in items {
        let line = format!("\n• [{}] {} ref:{}", item.class, item.summary, item.id);
        if used + line.len() > TELEGRAM_DIGEST_CAP {
            if delivered.is_empty() {
                // Single oversized item: bounded representation that still
                // advertises its resolvable detail reference. The prefix is
                // built from class + ref only, then a byte-bounded summary
                // slice is appended — never the whole oversized summary.
                let prefix = format!("\n• [{}] ref:{} ", item.class, item.id);
                let suffix = " (truncated; retrieve protected detail by ref)";
                let mut bounded = prefix.clone();
                for ch in item.summary.chars() {
                    if used + bounded.len() + ch.len_utf8() + suffix.len() > TELEGRAM_DIGEST_CAP {
                        break;
                    }
                    bounded.push(ch);
                }
                if used + bounded.len() + suffix.len() <= TELEGRAM_DIGEST_CAP {
                    bounded.push_str(suffix);
                    page.push_str(&bounded);
                    delivered.push(item.id);
                }
            }
            break;
        }
        used += line.len();
        page.push_str(&line);
        delivered.push(item.id);
    }
    (page, delivered)
}

/// Owner-authenticated retrieval of one failure's full detail. The digest
/// item is resolved by stable ID; its sensitive detail is decrypted only
/// from the encrypted artifact store (`text_ref`). Missing/corrupt/missing-
/// key/decrypt failures durably surface a Resource failure and fall back to
/// the bounded non-sensitive summary without leaking the cause. `failure
/// .digest_detail_viewed` is audited only after proven delivery.
pub(crate) async fn handle_detail_command(
    state: &AppState,
    owner_surface: &OwnerSurfaceRef,
    id: Ulid,
    page: usize,
) -> anyhow::Result<()> {
    let Some(item) = state.store.owner_digest_item(id)? else {
        super::notify_owner_best_effort(
            state,
            owner_surface,
            &format!("No failure record found for ref:{id}"),
        )
        .await;
        return Ok(());
    };
    let (body, total, detail_ref, unavailable) = match item.text_ref.as_deref() {
        None => {
            // Canonical unavailable markers are themselves terminal NULL-ref
            // rows. Viewing them must re-surface the non-secret message without
            // recursively inserting another marker/audit. Legacy NULL-ref rows
            // may still record one terminal marker via record_unavailable.
            if !state.store.is_canonical_unavailable_failure(&item) {
                record_unavailable(state)?;
            }
            (
                format!("detail unavailable [{}]", item.class),
                1,
                None,
                Some("legacy"),
            )
        }
        Some(ref_str) => match resolve_detail(state, ref_str) {
            Ok(detail) => {
                let pages = detail_pages(&detail);
                let total = pages.len();
                if page == 0 || page > total {
                    super::notify_owner_best_effort(
                        state,
                        owner_surface,
                        &format!("No detail page {page}; available pages: 1-{total}"),
                    )
                    .await;
                    return Ok(());
                }
                (pages[page - 1].clone(), total, Some(ref_str), None)
            }
            Err(_) => {
                record_unavailable(state)?;
                (
                    format!("detail unavailable [{}]", item.class),
                    1,
                    item.text_ref.as_deref(),
                    Some("unresolvable"),
                )
            }
        },
    };
    let message = format!(
        "Failure detail [{}] page {page}/{total}\n{}\nref:{}",
        item.class, body, item.id
    );
    // Carry the delivery's semantic metadata so a later dead-letter retry can
    // reconstruct the contract-specific receipt (identical to the immediate
    // path below).
    let detail = DetailReceipt {
        detail_ref: detail_ref.map(str::to_string),
        page_index: page,
        page_count: total,
        unavailable_reason: unavailable.map(str::to_string),
    };
    match super::notify_owner_with_digest(state, owner_surface, &message, &[], Some(&detail)).await
    {
        NotifyOutcome::Sent | NotifyOutcome::SendFailed => Ok(()),
        outcome => Err(anyhow::anyhow!(
            "owner detail notification failed: {outcome:?}"
        )),
    }
}

fn record_unavailable(state: &AppState) -> anyhow::Result<()> {
    // The "detail unavailable" marker is a non-secret constant; record it
    // directly without encrypting an artifact. This keeps surfacing the
    // unavailable state independent of the artifact store, which may be
    // inoperable (e.g. a crypto-erased counterparty or a key the kernel
    // cannot unwrap) -- the owner still learns the detail is unavailable and
    // nothing about the cause leaks.
    state.store.record_unavailable_failure("resource")?;
    Ok(())
}

fn detail_pages(detail: &str) -> Vec<String> {
    const PAGE_BUDGET: usize = TELEGRAM_DIGEST_CAP - 128;
    let mut pages = Vec::new();
    let mut start = 0;
    while start < detail.len() {
        let mut end = (start + PAGE_BUDGET).min(detail.len());
        while end > start && !detail.is_char_boundary(end) {
            end -= 1;
        }
        pages.push(detail[start..end].to_string());
        start = end;
    }
    if pages.is_empty() {
        pages.push(String::new());
    }
    pages
}

/// Resolve an encrypted `text_ref` back to its plaintext detail.
fn resolve_detail(state: &AppState, text_ref: &str) -> anyhow::Result<String> {
    let digest = Digest::parse(text_ref).map_err(|_| anyhow::anyhow!("invalid detail ref"))?;
    let bytes = state
        .artifacts
        .get(&ArtifactRef {
            digest,
            schema_version: 1,
        })
        .map_err(|e| anyhow::anyhow!("detail artifact unresolvable: {e}"))?;
    String::from_utf8(bytes).map_err(|_| anyhow::anyhow!("detail artifact not utf-8"))
}

pub(crate) async fn handle_command(
    state: &AppState,
    owner_surface: &OwnerSurfaceRef,
) -> anyhow::Result<()> {
    let items = state.store.owner_digest_items()?;
    let (digest, ids) = if items.is_empty() {
        ("Owner digest\nNo pending items.".to_string(), Vec::new())
    } else {
        render_page(&items)
    };
    let _ = super::notify_owner_with_digest(state, owner_surface, &digest, &ids, None).await;
    Ok(())
}

#[cfg(test)]
#[path = "digest_pagination_tests.rs"]
mod tests;
