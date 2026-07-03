//! Tests for `budget_support.rs`'s atomic grant counters. Split into its
//! own file (rather than folded into `store/tests.rs`) to keep both files
//! under the 500-line gate.

use openspine_schemas::artifact::ArtifactRef;
use openspine_schemas::digest::Digest;

use super::tests::sample_grant;
use super::Store;

#[test]
fn try_count_model_call_allows_exactly_one_concurrent_winner_at_max_one() {
    // Regression test for the TOCTOU gap closed by switching
    // `try_count_model_call` from a `SELECT COUNT` + app-side `< max`
    // compare to a single atomic `INSERT ... ON CONFLICT DO UPDATE ...
    // WHERE model_calls < ?2` upsert (see `budget_support.rs`). Under the
    // old racy implementation, N threads calling this concurrently on the
    // same `grant_id` with `max == 1` could all observe `count == 0` before
    // any of them wrote back, so more than one would be allowed through.
    //
    // We use real OS threads (`std::thread::spawn`), not tokio tasks on one
    // runtime — cooperative tokio scheduling on a single thread would never
    // actually interleave two SQL round-trips mid-flight the way genuine
    // parallel threads can, and would let a racy implementation pass by
    // accident.
    //
    // The whole spawn+join+assert is repeated many times (`ITERATIONS`)
    // rather than run once: a single run only proves the property held for
    // whatever thread interleaving the OS scheduler happened to produce
    // that time, and a lucky interleaving could make even the old racy code
    // pass by chance (e.g. if the OS never actually overlapped two threads'
    // critical sections). Repeating with a fresh grant/store each time and
    // requiring every iteration to hold makes a false pass from scheduling
    // luck astronomically unlikely without making the test flaky itself,
    // since each iteration is independent and a correct implementation
    // truly cannot violate the invariant no matter how threads interleave.
    const ITERATIONS: usize = 50;
    const THREADS: usize = 20;

    for _ in 0..ITERATIONS {
        let store = std::sync::Arc::new(Store::open_in_memory().unwrap());
        let grant = sample_grant("concurrent-model-call-token");
        let grant_id = grant.id;
        let pending_message_ref = ArtifactRef {
            digest: Digest::parse(format!("sha256:{}", "9".repeat(64))).unwrap(),
            schema_version: 1,
        };
        store
            .insert_task_grant(&grant, &pending_message_ref, 555)
            .unwrap();

        let handles: Vec<_> = (0..THREADS)
            .map(|_| {
                let store = std::sync::Arc::clone(&store);
                std::thread::spawn(move || store.try_count_model_call(grant_id, 1).unwrap())
            })
            .collect();

        let allowed = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .filter(|&allowed| allowed)
            .count();

        assert_eq!(
            allowed, 1,
            "expected exactly one of {THREADS} concurrent callers to be allowed under max=1"
        );
    }
}
