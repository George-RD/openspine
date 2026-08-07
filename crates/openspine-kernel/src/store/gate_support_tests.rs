use super::skill_read_queries::{insert_skill_context_selection, SkillContextSelection};
use super::Store;
use jiff::Timestamp;
use openspine_schemas::action::{ActionId, GateDecision};
use ulid::Ulid;

fn selection(grant_id: Ulid, expires_at: Timestamp) -> SkillContextSelection {
    SkillContextSelection {
        id: Ulid::new(),
        task_grant_id: grant_id,
        agent_id: "main_assistant_agent".to_string(),
        pack_id: "owner_default_pack".to_string(),
        skill_id: "skill".to_string(),
        skill_version: 1,
        task_class: "class".to_string(),
        expires_at,
        used: false,
    }
}

fn consume(store: &Store, selection: &SkillContextSelection, now: Timestamp) -> bool {
    store
        .consume_skill_context_selection_and_append_audit(
            selection.id,
            selection.task_grant_id,
            &selection.agent_id,
            &selection.pack_id,
            &ActionId::new("email.create_draft"),
            &GateDecision::Allow,
            &[],
            now,
        )
        .expect("consume")
}

/// A selection token whose `expires_at` lands on an exact second MUST be
/// refused one nanosecond later. `expires_at > ?5` is a `TEXT` comparison and
/// jiff's `Display` omits the fraction on a whole second, so the stored value
/// used to sort AFTER any sub-second `now` in the same second (`'Z'` 0x5A >
/// `'.'` 0x2E) — an expired token read as live for up to a second, a
/// fail-open window on token expiry. See `store::sql_time`.
#[test]
fn a_token_expiring_on_an_exact_second_is_refused_one_nanosecond_later() {
    let store = Store::open_in_memory().expect("test store");
    let grant_id = Ulid::new();
    let boundary = Timestamp::new(1_775_000_000, 0).expect("representable instant");
    let token = selection(grant_id, boundary);
    insert_skill_context_selection(&store, &token).expect("insert");

    let just_after = Timestamp::new(1_775_000_000, 1).expect("representable instant");
    assert!(
        !consume(&store, &token, just_after),
        "expiry on an exact second must not survive into the same second"
    );
}

/// The boundary is exclusive in the other direction too: one nanosecond
/// before expiry the token is still live, so the fix does not over-correct
/// into refusing tokens that have not expired.
#[test]
fn a_token_expiring_on_an_exact_second_is_live_one_nanosecond_earlier() {
    let store = Store::open_in_memory().expect("test store");
    let grant_id = Ulid::new();
    let boundary = Timestamp::new(1_775_000_000, 0).expect("representable instant");
    let token = selection(grant_id, boundary);
    insert_skill_context_selection(&store, &token).expect("insert");

    let just_before = Timestamp::new(1_774_999_999, 999_999_999).expect("representable instant");
    assert!(consume(&store, &token, just_before), "not yet expired");
}

/// A token is single-use regardless of the rendering change: the second
/// consume finds `used = 1` and fails closed.
#[test]
fn a_consumed_token_cannot_be_consumed_twice() {
    let store = Store::open_in_memory().expect("test store");
    let grant_id = Ulid::new();
    let now = Timestamp::new(1_775_000_000, 0).expect("representable instant");
    let token = selection(grant_id, now + std::time::Duration::from_secs(60));
    insert_skill_context_selection(&store, &token).expect("insert");

    assert!(consume(&store, &token, now));
    assert!(!consume(&store, &token, now), "single use");
}
