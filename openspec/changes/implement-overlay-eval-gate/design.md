# Design: Overlay evaluation gate

The artifact proposal dispatcher persists a proposal as `proposed`, validates it, and invokes `run_gate` before creating the digest-bound activation request. The gate requires positively identified owner-control conversation history and runs deterministic replay and risk-judge probes. Passing results are opaque proof values containing the evaluated artifact digest.

`Store::promote_authority_bearing_proposal` loads the proposal row inside one SQLite transaction, verifies `validated` state and both proof digests, inserts the replay and judge rows, and performs the only validated→review-required update. The generic state setter rejects that edge; inserts reject non-`proposed` initial state. The approval summary includes evaluator verdicts and evidence references; the full evidence JSON is also persisted in the eval store.

Current proposable kinds are all authority-bearing under D-048. No quiet-activating preference artifact exists in this codebase, so no bypass is exposed; a future non-authority kind requires a separate classified transition.

D-056 remains respected: evaluator metadata, open verdict labels, and evidence are landing metadata, not authority. Concrete independence and attack-trace policy remains deferred for owner ratification.
