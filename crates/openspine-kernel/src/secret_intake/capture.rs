use super::*;

/// Capture a pending message. Invalid/stale state is consumed and fails closed.
pub async fn capture(
    state: &AppState,
    chat_id: i64,
    text: &str,
) -> anyhow::Result<Option<CaptureOutcome>> {
    let Some(raw) = state.store.get_kv(PENDING_KEY)? else {
        return Ok(None);
    };
    let pending: Pending = match serde_json::from_str(&raw) {
        Ok(value) => value,
        Err(_) => {
            state.store.delete_kv(PENDING_KEY)?;
            state.store.append_audit(
                "secret.intake.rejected",
                None,
                None,
                Some("pending record invalid"),
                None,
                &[],
                &[],
            )?;
            return Ok(Some(CaptureOutcome::Rejected));
        }
    };
    let kind_prefix = kind_prefix(pending.mode);
    let correlation = format!(
        "slot={}; action_request_id={}",
        pending.slot, pending.action_request_id
    );
    let now = Timestamp::now();
    let target_matches = digest_of_bytes(pending.slot.as_bytes()) == pending.target_digest;
    if pending.chat_id != chat_id || pending.expires_at <= now || !target_matches {
        state.store.delete_kv(PENDING_KEY)?;
        let reason = if !target_matches {
            "pending target binding invalid"
        } else if pending.chat_id != chat_id {
            "pending chat binding invalid"
        } else {
            "pending capture expired"
        };
        state.store.append_audit(
            &format!("{kind_prefix}.rejected"),
            None,
            None,
            Some(&format!("{reason}; {correlation}")),
            Some(pending.grant_id),
            &[],
            &[],
        )?;
        return Ok(Some(CaptureOutcome::Rejected));
    }
    state.store.delete_kv(PENDING_KEY)?;

    if pending.slot == crate::telegram::BOT_TOKEN_SLOT {
        crate::spend::guard_connector(state, true).await?;
        let Some(bot_id) = state
            .connectors
            .telegram()
            .validate_candidate_token_id(text)
            .await
        else {
            state.store.append_audit(
                &format!("{kind_prefix}.rejected"),
                None,
                None,
                Some(&format!("candidate token validation failed; {correlation}")),
                Some(pending.grant_id),
                &[],
                &[],
            )?;
            return Ok(Some(CaptureOutcome::Rejected));
        };
        let previous = state.secrets.get(&pending.slot)?;
        let previous_bot_id = state.store.get_kv("telegram.bot_id")?;
        state.secrets.put(&pending.slot, text.as_bytes())?;
        let restore_bot_id = || -> anyhow::Result<()> {
            match &previous_bot_id {
                Some(value) => state.store.set_kv("telegram.bot_id", value),
                None => state.store.delete_kv("telegram.bot_id"),
            }
            .map_err(Into::into)
        };
        if let Err(bot_id_err) = state.store.set_kv("telegram.bot_id", &bot_id.to_string()) {
            let token_rollback = rollback_secret(&state.secrets, &pending.slot, previous);
            let bot_id_rollback = restore_bot_id();
            if let Err(rb) = token_rollback {
                anyhow::bail!(
                    "bot-id transition failed ({bot_id_err}); token rollback failed ({rb})"
                );
            }
            if let Err(rb) = bot_id_rollback {
                anyhow::bail!(
                    "bot-id transition failed ({bot_id_err}); bot-id rollback failed ({rb})"
                );
            }
            return Err(bot_id_err.into());
        }
        if let Err(audit_err) = state.store.append_audit(
            &format!("{kind_prefix}.stored"),
            Some(&action_for(pending.mode)),
            Some(&GateDecision::Allow),
            Some(&correlation),
            Some(pending.grant_id),
            &[],
            &[],
        ) {
            let token_rollback = rollback_secret(&state.secrets, &pending.slot, previous);
            let bot_id_rollback = restore_bot_id();
            if let Err(rb) = token_rollback {
                anyhow::bail!("audit failed ({audit_err}); token rollback failed ({rb})");
            }
            if let Err(rb) = bot_id_rollback {
                anyhow::bail!("audit failed ({audit_err}); bot-id rollback failed ({rb})");
            }
            return Err(audit_err.into());
        }
        return Ok(Some(CaptureOutcome::Stored(pending.mode)));
    }

    let (counterpart, is_gmail) = match pending.slot.as_str() {
        "gmail.refresh_token" => ("gmail.client_secret", true),
        "gmail.client_secret" => ("gmail.refresh_token", true),
        _ => ("", false),
    };
    if is_gmail {
        let staged_counterpart_slot = format!("secret.staged.{counterpart}");
        let stage_meta_key = format!("secret.stage.{counterpart}");
        let own_meta_key = format!("secret.stage.{}", pending.slot);
        let raw_stage_meta = state.store.get_kv(&stage_meta_key)?;
        let stage_meta_fresh = raw_stage_meta
            .as_deref()
            .and_then(|s| serde_json::from_str::<StageMeta>(s).ok())
            .is_some_and(|m| {
                m.expires_at > now
                    && m.counterpart_slot == pending.slot
                    && m.mode == pending.mode
                    && pending.stage_correlation_id == Some(m.correlation_id)
            });
        let has_staged_counterpart = state.secrets.contains(&staged_counterpart_slot)?;

        if has_staged_counterpart && stage_meta_fresh {
            let Some(gmail) = state.connectors.gmail() else {
                state.store.append_audit(
                    &format!("{kind_prefix}.rejected"),
                    None,
                    None,
                    Some(&format!("gmail connector unavailable; {correlation}")),
                    Some(pending.grant_id),
                    &[],
                    &[],
                )?;
                return Ok(Some(CaptureOutcome::Rejected));
            };
            let Some(staged_value) = state.secrets.get_string(&staged_counterpart_slot)? else {
                state.store.append_audit(
                    &format!("{kind_prefix}.rejected"),
                    None,
                    None,
                    Some(&format!("staged counterpart unreadable; {correlation}")),
                    Some(pending.grant_id),
                    &[],
                    &[],
                )?;
                return Ok(Some(CaptureOutcome::Rejected));
            };
            let (client_secret, refresh_token) = if pending.slot == "gmail.client_secret" {
                (text.to_string(), staged_value.clone())
            } else {
                (staged_value.clone(), text.to_string())
            };
            crate::spend::guard_connector(state, true).await?;
            if !gmail
                .validate_credential_pair(&client_secret, &refresh_token)
                .await
            {
                state.store.append_audit(
                    &format!("{kind_prefix}.rejected"),
                    None,
                    None,
                    Some(&format!("paired validation failed; {correlation}")),
                    Some(pending.grant_id),
                    &[],
                    &[],
                )?;
                return Ok(Some(CaptureOutcome::Rejected));
            }
            // Full pre-mutation snapshot so any failure after the first write
            // rolls back every component (both live slots, staged credential
            // value, and staging metadata) to exactly the pre-promotion state.
            let prev_candidate = state.secrets.get(&pending.slot)?;
            let prev_counterpart = state.secrets.get(counterpart)?;
            let prev_staged = state.secrets.get(&staged_counterpart_slot)?;
            let prev_meta = raw_stage_meta.clone();
            let restore = || -> Vec<String> {
                let mut errs = Vec::new();
                match &prev_candidate {
                    Some(v) => {
                        if let Err(e) = state.secrets.put(&pending.slot, v) {
                            errs.push(format!("candidate: {e}"));
                        }
                    }
                    None => {
                        if let Err(e) = state.secrets.delete(&pending.slot) {
                            errs.push(format!("candidate delete: {e}"));
                        }
                    }
                }
                match &prev_counterpart {
                    Some(v) => {
                        if let Err(e) = state.secrets.put(counterpart, v) {
                            errs.push(format!("counterpart: {e}"));
                        }
                    }
                    None => {
                        if let Err(e) = state.secrets.delete(counterpart) {
                            errs.push(format!("counterpart delete: {e}"));
                        }
                    }
                }
                match &prev_staged {
                    Some(v) => {
                        if let Err(e) = state.secrets.put(&staged_counterpart_slot, v) {
                            errs.push(format!("staged: {e}"));
                        }
                    }
                    None => {
                        if let Err(e) = state.secrets.delete(&staged_counterpart_slot) {
                            errs.push(format!("staged delete: {e}"));
                        }
                    }
                }
                match &prev_meta {
                    Some(m) => {
                        if let Err(e) = state.store.set_kv(&stage_meta_key, m) {
                            errs.push(format!("meta: {e}"));
                        }
                    }
                    None => {
                        if let Err(e) = state.store.delete_kv(&stage_meta_key) {
                            errs.push(format!("meta delete: {e}"));
                        }
                    }
                }
                errs
            };
            macro_rules! bail_with_rollback {
                ($step:expr, $err:expr) => {{
                    let errs = restore();
                    anyhow::bail!(
                        "paired promotion failed at {} ({}); rollback: {}",
                        $step,
                        $err,
                        if errs.is_empty() {
                            "ok".to_string()
                        } else {
                            errs.join("; ")
                        }
                    );
                }};
            }
            if let Err(e) = state.secrets.put(&pending.slot, text.as_bytes()) {
                bail_with_rollback!("candidate put", e);
            }
            if let Err(e) = state.secrets.put(counterpart, staged_value.as_bytes()) {
                bail_with_rollback!("counterpart put", e);
            }
            if let Err(e) = state.secrets.delete(&staged_counterpart_slot) {
                bail_with_rollback!("staged delete", e);
            }
            if let Err(e) = state.store.delete_kv(&stage_meta_key) {
                bail_with_rollback!("meta delete", e);
            }
            if let Err(audit_err) = state.store.append_audit(
                &format!("{kind_prefix}.stored"),
                Some(&action_for(pending.mode)),
                Some(&GateDecision::Allow),
                Some(&format!("{correlation}; paired_promotion=true")),
                Some(pending.grant_id),
                &[],
                &[],
            ) {
                bail_with_rollback!("audit append", audit_err);
            }
            return Ok(Some(CaptureOutcome::Stored(pending.mode)));
        }
        if has_staged_counterpart && !stage_meta_fresh {
            let _ = state.secrets.delete(&staged_counterpart_slot);
            let _ = state.store.delete_kv(&stage_meta_key);
        }
        if state.secrets.contains(counterpart)? {
            let Some(gmail) = state.connectors.gmail() else {
                state.store.append_audit(
                    &format!("{kind_prefix}.rejected"),
                    None,
                    None,
                    Some(&format!("gmail connector unavailable; {correlation}")),
                    Some(pending.grant_id),
                    &[],
                    &[],
                )?;
                return Ok(Some(CaptureOutcome::Rejected));
            };
            let live_counterpart = state.secrets.get_string(counterpart)?.unwrap_or_default();
            let (client_secret, refresh_token) = if pending.slot == "gmail.client_secret" {
                (text.to_string(), live_counterpart)
            } else {
                (live_counterpart, text.to_string())
            };
            crate::spend::guard_connector(state, true).await?;
            if !gmail
                .validate_credential_pair(&client_secret, &refresh_token)
                .await
            {
                state.store.append_audit(
                    &format!("{kind_prefix}.rejected"),
                    None,
                    None,
                    Some(&format!("candidate validation failed; {correlation}")),
                    Some(pending.grant_id),
                    &[],
                    &[],
                )?;
                return Ok(Some(CaptureOutcome::Rejected));
            }
            let previous = state.secrets.get(&pending.slot)?;
            state.secrets.put(&pending.slot, text.as_bytes())?;
            if let Err(audit_err) = state.store.append_audit(
                &format!("{kind_prefix}.stored"),
                Some(&action_for(pending.mode)),
                Some(&GateDecision::Allow),
                Some(&correlation),
                Some(pending.grant_id),
                &[],
                &[],
            ) {
                rollback_secret(&state.secrets, &pending.slot, previous)?;
                return Err(audit_err.into());
            }
            return Ok(Some(CaptureOutcome::Stored(pending.mode)));
        }
        let stage_meta = StageMeta {
            correlation_id: Ulid::new(),
            grant_id: pending.grant_id,
            mode: pending.mode,
            counterpart_slot: counterpart.to_string(),
            staged_at: now,
            expires_at: now + std::time::Duration::from_secs(STAGE_TTL_SECONDS as u64),
        };
        let staged_slot = format!("secret.staged.{}", pending.slot);
        let previous = state.secrets.get(&staged_slot)?;
        state.secrets.put(&staged_slot, text.as_bytes())?;
        state
            .store
            .set_kv(&own_meta_key, &serde_json::to_string(&stage_meta)?)?;
        if let Err(audit_err) = state.store.append_audit(
            &format!("{kind_prefix}.staged"),
            Some(&action_for(pending.mode)),
            Some(&GateDecision::Allow),
            Some(&format!("{correlation}; awaiting_pair={counterpart}")),
            Some(pending.grant_id),
            &[],
            &[],
        ) {
            rollback_secret(&state.secrets, &staged_slot, previous)?;
            let _ = state.store.delete_kv(&own_meta_key);
            return Err(audit_err.into());
        }
        return Ok(Some(CaptureOutcome::Staged(pending.mode)));
    }

    let previous = state.secrets.get(&pending.slot)?;
    state.secrets.put(&pending.slot, text.as_bytes())?;
    if let Err(audit_err) = state.store.append_audit(
        &format!("{kind_prefix}.stored"),
        Some(&action_for(pending.mode)),
        Some(&GateDecision::Allow),
        Some(&correlation),
        Some(pending.grant_id),
        &[],
        &[],
    ) {
        rollback_secret(&state.secrets, &pending.slot, previous)?;
        return Err(audit_err.into());
    }
    Ok(Some(CaptureOutcome::Stored(pending.mode)))
}
