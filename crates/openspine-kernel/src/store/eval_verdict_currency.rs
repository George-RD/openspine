//! Activation-time verdict currency (#133).
//!
//! A verdict earned at propose time says nothing about the world at
//! activation time. Between the two, a descriptor can be revised, an executor
//! deregistered, a policy version bumped, or the reviewed scope re-derived —
//! and #128 already proved that a required-dimension change moves the
//! compatibility epoch. Activation therefore re-checks that every epoch the
//! stored verdicts recorded still equals the live value, and refuses when it
//! does not.
//!
//! Currency is decided here from stored columns against live values. Nothing
//! sweeps and nothing rewrites a row, so there is no window in which a stale
//! verdict still reads fresh and no crash can strand a half-finished pass.

use openspine_schemas::standing_rule::StandingRuleManifest;

use super::eval_verdict_store::VerdictEpochs;
use super::{Store, StoreError};

impl Store {
    /// The epochs a standing-rule manifest binds right now.
    ///
    /// Known limit, recorded rather than hidden: `compatibility_digest` and
    /// `reviewed_scope_digest` are read back from the same manifest being
    /// activated, so those two axes cannot detect movement on their own — the
    /// manifest carries whatever it was proposed with. What does move is the
    /// descriptor, implementation and policy versions beside them, which is
    /// how catalog and policy drift is actually caught here. Detecting a
    /// re-derived scope would require rebuilding the resolved context at
    /// activation, which needs a live grant this path does not have.
    pub(crate) fn live_epochs_for_standing_rule(
        &self,
        manifest: &StandingRuleManifest,
        descriptor_version: Option<u32>,
        implementation_version: Option<u32>,
        policy_version: Option<u32>,
    ) -> VerdictEpochs {
        let binding = manifest.reviewed_scope.as_ref();
        VerdictEpochs {
            // The proposal digest is not re-derivable here (the YAML bytes
            // live in the artifact store), so it is not compared at
            // activation; `promote_authority_bearing_proposal` already bound
            // it and refuses a mismatched token.
            proposal_digest: None,
            compatibility_digest: binding
                .map(|binding| binding.compatibility_digest.as_str().to_string()),
            reviewed_scope_digest: binding
                .map(|binding| binding.reviewed_scope_digest.as_str().to_string()),
            evidence_set_digest: None,
            descriptor_version,
            implementation_version,
            policy_version,
        }
    }

    /// Refuse activation when the latest stored verdicts for this artifact no
    /// longer bind the live epochs. Runs before the activation transaction so
    /// the durable refusal audit survives the refusal.
    pub(crate) fn reject_stale_eval_verdicts(
        &self,
        kind: &str,
        artifact_id: &str,
        version: u32,
        live: &VerdictEpochs,
    ) -> Result<(), StoreError> {
        let verdicts = self.eval_verdicts_for_artifact(kind, artifact_id, version)?;
        // A pre-#133 verdict recorded no epochs at all; it is compared on no
        // axis and so cannot be stale. Only what a verdict actually bound is
        // ever checked against the world.
        let mut stale_axes: Vec<&'static str> = Vec::new();
        for verdict in &verdicts {
            // Activation cannot re-derive the proposal digest — the YAML
            // bytes live in the artifact store — so that axis is projected
            // out of the comparison rather than being reported as an axis
            // that disappeared. `promote_authority_bearing_proposal` already
            // bound it and refuses a token whose digest does not match the
            // stored row, so it is checked, just not here.
            let mut recorded = verdict.epochs.clone();
            recorded.proposal_digest = None;
            for axis in recorded.stale_axes(live) {
                if !stale_axes.contains(&axis) {
                    stale_axes.push(axis);
                }
            }
        }
        if stale_axes.is_empty() {
            return Ok(());
        }
        let reason = format!(
            "evaluation verdicts for {kind} {artifact_id} v{version} are stale on: {}",
            stale_axes.join(", ")
        );
        self.append_audit(
            "eval_verdict.stale_at_activation",
            None,
            None,
            Some(&reason),
            None,
            &[],
            &[],
        )?;
        Err(StoreError::ProposedArtifactLifecycle(reason))
    }
}
