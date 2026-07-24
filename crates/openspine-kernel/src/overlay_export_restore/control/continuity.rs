//! Generation continuity, terminal-erasure ledger, and portable continuity
//! methods for OverlayControl.

use std::collections::BTreeSet;

use ulid::Ulid;

use super::fs::atomic_write;
use super::generation::{
    check_continuity_alignment, ensure_data_root_dir, read_generation_marker,
    write_generation_marker,
};
use super::ledger::{newer_ledger, validate_counterparty_id, validate_ledger};
use super::wire::{
    io, mac_hex, malformed, master_key_context, verify_mac, PortableContinuity,
    PortableContinuityBody, SignedTerminalLedger, TerminalLedger, FORMAT_VERSION, LEDGER_DOMAIN,
    LEDGER_FILE, LEDGER_TEMP, PORTABLE_DOMAIN,
};
use super::ControlError;
use super::OverlayControl;

impl OverlayControl {
    pub(crate) fn ensure_data_root_for_first_boot(&self) -> Result<(), ControlError> {
        let _g = self.state.lock().unwrap_or_else(|p| p.into_inner());

        ensure_data_root_dir(&self.canonical_data_root)?;

        let marker = read_generation_marker(&self.canonical_data_root, &self.master_key)?;
        let ledger = self.load_ledger_unlocked()?;

        match (marker.as_deref(), ledger.as_ref()) {
            (Some(m), Some(l)) => {
                if m != l.continuity_id() {
                    return Err(ControlError::RegressedContinuity);
                }
            }
            (Some(_), None) => return Err(ControlError::MissingContinuity),
            (None, Some(l)) => {
                write_generation_marker(
                    &self.canonical_data_root,
                    l.continuity_id(),
                    &self.master_key,
                )?;
            }
            (None, None) => {
                let init = self.sign_ledger(TerminalLedger::with_continuity_id(
                    Ulid::new().to_string(),
                    0,
                    BTreeSet::new(),
                ))?;
                self.write_ledger(&init)?;
                #[cfg(test)]
                if self
                    .fail_before_first_boot_marker
                    .swap(false, std::sync::atomic::Ordering::SeqCst)
                {
                    return Err(ControlError::Io {
                        path: self.canonical_data_root.clone(),
                        source: std::io::Error::other("failpoint_before_first_boot_marker"),
                    });
                }
                write_generation_marker(
                    &self.canonical_data_root,
                    init.continuity_id(),
                    &self.master_key,
                )?;
            }
        }
        Ok(())
    }

    pub(crate) fn initialize_terminal_ledger(&self) -> Result<SignedTerminalLedger, ControlError> {
        let _g = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if !self.canonical_data_root.exists() {
            return Err(ControlError::MissingContinuity);
        }
        let ledger = self.load_ledger_unlocked()?;
        let marker = read_generation_marker(&self.canonical_data_root, &self.master_key)?;

        match (marker.as_deref(), ledger.as_ref()) {
            (Some(m), Some(l)) => {
                if m != l.continuity_id() {
                    return Err(ControlError::RegressedContinuity);
                }
            }
            (Some(_), None) => return Err(ControlError::MissingContinuity),
            (None, Some(l)) => {
                write_generation_marker(
                    &self.canonical_data_root,
                    l.continuity_id(),
                    &self.master_key,
                )?;
            }
            (None, None) => {}
        }

        if let Some(l) = ledger {
            return Ok(l);
        }

        let init = self.sign_ledger(TerminalLedger::with_continuity_id(
            Ulid::new().to_string(),
            0,
            BTreeSet::new(),
        ))?;
        self.write_ledger(&init)?;
        #[cfg(test)]
        if self
            .fail_before_init_ledger_marker
            .swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            return Err(ControlError::Io {
                path: self.canonical_data_root.clone(),
                source: std::io::Error::other("failpoint_before_init_ledger_marker"),
            });
        }
        write_generation_marker(
            &self.canonical_data_root,
            init.continuity_id(),
            &self.master_key,
        )?;
        Ok(init)
    }

    pub(crate) fn record_terminal_erasure(
        &self,
        counterparty_id: &str,
    ) -> Result<SignedTerminalLedger, ControlError> {
        validate_counterparty_id(counterparty_id)?;
        let _g = self.state.lock().unwrap_or_else(|p| p.into_inner());

        let marker = read_generation_marker(&self.canonical_data_root, &self.master_key)?;
        let ledger = self.load_ledger_unlocked()?;

        check_continuity_alignment(marker.as_deref(), ledger.as_ref())?;

        let mut body = ledger.ok_or(ControlError::MissingContinuity)?.body;
        if !body
            .erased_counterparty_ids
            .insert(counterparty_id.to_owned())
        {
            return self.sign_ledger(body);
        }
        body.sequence = body
            .sequence
            .checked_add(1)
            .ok_or(ControlError::SequenceOverflow)?;
        let signed = self.sign_ledger(body)?;
        self.write_ledger(&signed)?;
        Ok(signed)
    }

    pub(crate) fn export_terminal_ledger(&self) -> Result<SignedTerminalLedger, ControlError> {
        let _g = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let marker = read_generation_marker(&self.canonical_data_root, &self.master_key)?;
        let ledger = self.load_ledger_unlocked()?;

        check_continuity_alignment(marker.as_deref(), ledger.as_ref())?;

        ledger.ok_or(ControlError::MissingContinuity)
    }

    pub(crate) fn export_portable_continuity(&self) -> Result<Vec<u8>, ControlError> {
        let ledger = self.export_terminal_ledger()?;
        self.encode_portable(ledger)
    }

    pub(crate) fn import_terminal_ledger(
        &self,
        portable: &[u8],
    ) -> Result<SignedTerminalLedger, ControlError> {
        let imported = self.decode_portable(portable)?;
        let _g = self.state.lock().unwrap_or_else(|p| p.into_inner());

        if !self.canonical_data_root.exists() {
            ensure_data_root_dir(&self.canonical_data_root)?;
        }
        let marker = read_generation_marker(&self.canonical_data_root, &self.master_key)?;
        if let Some(m) = marker.as_deref() {
            if m != imported.continuity_id() {
                return Err(ControlError::RegressedContinuity);
            }
        }

        let merged = match self.load_ledger_unlocked()? {
            Some(local) => newer_ledger(local, imported)?,
            None => imported,
        };
        self.write_ledger(&merged)?;
        write_generation_marker(
            &self.canonical_data_root,
            merged.continuity_id(),
            &self.master_key,
        )?;
        Ok(merged)
    }

    pub(crate) fn merge_bundle_baseline(
        &self,
        baseline: &SignedTerminalLedger,
        portable: &[u8],
    ) -> Result<SignedTerminalLedger, ControlError> {
        if portable.is_empty() {
            return Err(ControlError::MissingContinuity);
        }
        self.verify_ledger(baseline)?;
        let imported = self.decode_portable(portable)?;
        let _g = self.state.lock().unwrap_or_else(|p| p.into_inner());

        let local = self
            .load_ledger_unlocked()?
            .ok_or(ControlError::MissingContinuity)?;
        let continuity = newer_ledger(local, imported).map_err(|error| match error {
            ControlError::DivergedContinuity => ControlError::RegressedContinuity,
            other => other,
        })?;
        if continuity.body.continuity_id != baseline.body.continuity_id
            || continuity.body.sequence < baseline.body.sequence
            || !continuity
                .body
                .erased_counterparty_ids
                .is_superset(&baseline.body.erased_counterparty_ids)
        {
            return Err(ControlError::RegressedContinuity);
        }
        let merged = newer_ledger(baseline.clone(), continuity).map_err(|error| match error {
            ControlError::DivergedContinuity => ControlError::RegressedContinuity,
            other => other,
        })?;

        self.write_ledger(&merged)?;
        if self.canonical_data_root.exists() {
            write_generation_marker(
                &self.canonical_data_root,
                merged.continuity_id(),
                &self.master_key,
            )?;
        }
        Ok(merged)
    }

    pub(crate) fn sign_ledger(
        &self,
        body: TerminalLedger,
    ) -> Result<SignedTerminalLedger, ControlError> {
        validate_ledger(&body)?;
        let canonical = serde_json::to_vec(&body).map_err(malformed)?;
        Ok(SignedTerminalLedger {
            body,
            hmac_sha256: mac_hex(LEDGER_DOMAIN, &canonical, &self.master_key),
        })
    }

    fn load_ledger_unlocked(&self) -> Result<Option<SignedTerminalLedger>, ControlError> {
        let path = self.control_root.join(LEDGER_FILE);
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(source) => return Err(io(&path, source)),
        };
        let ledger: SignedTerminalLedger = serde_json::from_slice(&bytes).map_err(malformed)?;
        self.verify_ledger(&ledger)?;
        Ok(Some(ledger))
    }

    fn verify_ledger(&self, ledger: &SignedTerminalLedger) -> Result<(), ControlError> {
        validate_ledger(&ledger.body)?;
        let canonical = serde_json::to_vec(&ledger.body).map_err(malformed)?;
        verify_mac(
            LEDGER_DOMAIN,
            &canonical,
            &self.master_key,
            &ledger.hmac_sha256,
        )
    }

    fn write_ledger(&self, ledger: &SignedTerminalLedger) -> Result<(), ControlError> {
        atomic_write(
            &self.control_root,
            LEDGER_TEMP,
            LEDGER_FILE,
            &serde_json::to_vec(ledger).map_err(malformed)?,
        )
    }

    pub(crate) fn encode_portable(
        &self,
        ledger: SignedTerminalLedger,
    ) -> Result<Vec<u8>, ControlError> {
        self.verify_ledger(&ledger)?;
        let body = PortableContinuityBody {
            version: FORMAT_VERSION,
            master_key_context: master_key_context(&self.master_key),
            ledger,
        };
        let canonical = serde_json::to_vec(&body).map_err(malformed)?;
        let portable = PortableContinuity {
            body,
            hmac_sha256: mac_hex(PORTABLE_DOMAIN, &canonical, &self.master_key),
        };
        serde_json::to_vec(&portable).map_err(malformed)
    }

    fn decode_portable(&self, bytes: &[u8]) -> Result<SignedTerminalLedger, ControlError> {
        let portable: PortableContinuity = serde_json::from_slice(bytes).map_err(malformed)?;
        if portable.body.version != FORMAT_VERSION {
            return Err(ControlError::MalformedState(
                "unsupported portable continuity version".into(),
            ));
        }
        if portable.body.master_key_context != master_key_context(&self.master_key) {
            return Err(ControlError::AuthenticationFailed);
        }
        let canonical = serde_json::to_vec(&portable.body).map_err(malformed)?;
        verify_mac(
            PORTABLE_DOMAIN,
            &canonical,
            &self.master_key,
            &portable.hmac_sha256,
        )?;
        self.verify_ledger(&portable.body.ledger)?;
        Ok(portable.body.ledger)
    }
}
