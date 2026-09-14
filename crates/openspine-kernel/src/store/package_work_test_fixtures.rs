fn insert_reconfirmation(store: &Store, as_of: Timestamp) {
    use openspine_schemas::action::ActionRequest;
    use openspine_schemas::artifact::ArtifactRef;
    use openspine_schemas::digest::digest_of_bytes;
    let request = ActionRequest {
        id: ulid::Ulid::new(),
        task_grant_id: ulid::Ulid::new(),
        action: "artifact.reconfirm".into(),
        target_ref: None,
        payload_ref: Some(ArtifactRef {
            digest: digest_of_bytes(b"census reconfirmation"),
            schema_version: 1,
        }),
        target_digest: Some(digest_of_bytes(b"census target")),
        selection_token_id: None,
        params: Default::default(),
        skill_attribution: None,
        requested_at: as_of,
        schema_version: 1,
    };
    store.insert_action_request(&request).unwrap();
}
