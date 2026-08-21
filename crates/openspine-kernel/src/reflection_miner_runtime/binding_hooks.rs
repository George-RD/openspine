use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use crate::api::artifact_propose::ArtifactProposalReceipt;
use crate::pipeline::AppState;
use rusqlite::params;

#[derive(Clone, Copy, Debug)]
pub(crate) enum DispatchTestMutation {
    MissingVerdict,
    MismatchedDigest,
    NonReviewRequired,
    StaleEpoch,
    DeniedVerdict,
}

static MUTATIONS: LazyLock<Mutex<HashMap<usize, DispatchTestMutation>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn mutation_key(state: &AppState) -> usize {
    state as *const AppState as usize
}

pub(crate) fn set_dispatch_test_mutation(state: &AppState, mutation: Option<DispatchTestMutation>) {
    let mut mutations = MUTATIONS.lock().expect("dispatch mutation lock");
    let key = mutation_key(state);
    if let Some(mutation) = mutation {
        mutations.insert(key, mutation);
    } else {
        mutations.remove(&key);
    }
}

pub(crate) fn apply_dispatch_test_mutation(state: &AppState, receipt: &ArtifactProposalReceipt) {
    let mutation = MUTATIONS
        .lock()
        .expect("dispatch mutation lock")
        .remove(&mutation_key(state));
    let Some(mutation) = mutation else {
        return;
    };
    state.store.with_conn_for_test(|conn| match mutation {
        DispatchTestMutation::MissingVerdict => {
            conn.execute(
                "DELETE FROM eval_verdicts WHERE id = ?1",
                params![receipt.replay_verdict_id.to_string()],
            )
            .expect("delete replay verdict");
        }
        DispatchTestMutation::MismatchedDigest => {
            conn.execute(
                "UPDATE eval_verdicts SET artifact_digest = ?2 WHERE id = ?1",
                params![
                    receipt.replay_verdict_id.to_string(),
                    "sha256:0000000000000000000000000000000000000000000000000000000000000000"
                ],
            )
            .expect("mutate replay verdict digest");
        }
        DispatchTestMutation::NonReviewRequired => {
            conn.execute(
                "UPDATE proposed_artifacts SET state = 'approved' WHERE id = ?1",
                params![receipt.proposal_id.to_string()],
            )
            .expect("mutate proposal lifecycle state");
        }
        DispatchTestMutation::StaleEpoch => {
            conn.execute(
                "UPDATE eval_verdicts SET descriptor_version = 999 WHERE id = ?1",
                params![receipt.replay_verdict_id.to_string()],
            )
            .expect("mutate replay verdict epoch");
        }
        DispatchTestMutation::DeniedVerdict => {
            conn.execute(
                "UPDATE eval_verdicts SET verdict = 'denied' WHERE id = ?1",
                params![receipt.replay_verdict_id.to_string()],
            )
            .expect("mutate replay verdict decision");
        }
    });
}
