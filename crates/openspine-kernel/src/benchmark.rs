use crate::store::Store;
use openspine_schemas::action::{ActionId, DenialReason, GateDecision};
use openspine_schemas::artifact::ArtifactRef;
use openspine_schemas::digest::Digest;
use std::time::Instant;

pub fn run_benchmarks() -> anyhow::Result<()> {
    println!("Starting benchmarks...");

    // 1. Digest Benchmark
    let sample_json = serde_json::json!({
        "id": "01JM4X2Z0G8Q6R8S9T0V1W2X3Y",
        "ts": "2026-07-05T16:34:00Z",
        "kind": "email.send_draft",
        "action": {
            "pack_id": "lyra.email",
            "action_name": "send"
        },
        "decision": {
            "effect": "ApprovalRequired",
            "reason": "sensitive action requires owner confirmation"
        },
        "reason": "Owner requested to send a drafted response to client",
        "task_grant_id": "01JM4X2Z0G8Q6R8S9T0V1W2X3A",
        "target_refs": [
            {
                "digest": "sha256:48da91e7993d56cb98fcc5a422899206556514d475ffc43eb4d1f20337ea3695",
                "schema_version": 1
            },
            {
                "digest": "sha256:b288e4ab449602b3e94c6fbb6e12ecf30651ab98102615dd592bf9ca79f0606b",
                "schema_version": 1
            }
        ],
        "payload_refs": [
            {
                "digest": "sha256:de49e245261c38c777db5290324ed8773176ffa7440a3f16fabaf1bba46da501",
                "schema_version": 1
            }
        ],
        "nested_details": {
            "attempts": 3,
            "status": "pending",
            "errors": [],
            "metadata": {
                "user_agent": "Mozilla/5.0",
                "ip": "127.0.0.1",
                "origin": "telegram_bot"
            }
        }
    });

    let digest_iterations = 20000;
    let start_digest = Instant::now();
    for _ in 0..digest_iterations {
        let _digest = openspine_schemas::digest::digest_of(&sample_json);
    }
    let duration_digest = start_digest.elapsed();
    let digest_ns = duration_digest.as_nanos() / digest_iterations as u128;
    println!("digest_ns={}", digest_ns);

    // 2. Audit Chain Verification Benchmark
    let store = Store::open_in_memory()?;

    let target_ref = ArtifactRef {
        digest: Digest::parse(
            "sha256:48da91e7993d56cb98fcc5a422899206556514d475ffc43eb4d1f20337ea3695".to_string(),
        )
        .unwrap(),
        schema_version: 1,
    };
    let payload_ref = ArtifactRef {
        digest: Digest::parse(
            "sha256:de49e245261c38c777db5290324ed8773176ffa7440a3f16fabaf1bba46da501".to_string(),
        )
        .unwrap(),
        schema_version: 1,
    };

    let audit_count = 20000;
    println!("Inserting {} audit events...", audit_count);
    let start_insert = Instant::now();
    for _ in 0..audit_count {
        store.append_audit(
            "action.gate_decision",
            Some(&ActionId::new("openspine.status.read")),
            Some(&GateDecision::Deny {
                reason: DenialReason::NotGranted,
            }),
            Some("not_granted"),
            None,
            &[target_ref.clone()],
            &[payload_ref.clone()],
        )?;
    }
    let duration_insert = start_insert.elapsed();
    println!("Inserted in {:?}", duration_insert);

    println!("Verifying audit chain...");
    let start_verify = Instant::now();
    let ok = store.verify_audit_chain()?;
    let duration_verify = start_verify.elapsed();
    assert!(ok, "Audit chain verification must succeed");

    let verify_audit_chain_ms = duration_verify.as_millis();
    println!("verify_audit_chain_ms={}", verify_audit_chain_ms);

    println!("METRIC verify_audit_chain_ms={}", verify_audit_chain_ms);
    println!("METRIC digest_ns={}", digest_ns);

    Ok(())
}
