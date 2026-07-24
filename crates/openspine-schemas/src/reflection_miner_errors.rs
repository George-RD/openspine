#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum MinerError {
    #[error("miner briefcase scope must not be empty")]
    EmptyScope,
    #[error("briefcase entry is outside its declared scope")]
    BriefcaseScopeMismatch,
    #[error("briefcase belongs to a different task grant")]
    BriefcaseGrantMismatch,
    #[error("miner grant is expired")]
    GrantExpired,
    #[error("miner grants must have empty output_channels")]
    OutputChannelsNotEmpty,
    #[error("miner grant is missing model.generate:approved_provider")]
    RequiredActionMissing,
    #[error("miner grant contains a direct mutation action")]
    DirectMutationAction,
    #[error("audit slice exceeds the pack classification ceiling")]
    ClassificationExceeded,
    #[error("observation provenance is outside the scoped audit briefcase")]
    ProvenanceOutOfScope,
    #[error("artifact proposal limit exhausted")]
    ArtifactLimitExceeded,
    #[error("correction requires a positive instruction and reason")]
    EmptyCorrection,
    #[error("repeated approval requires at least two audit entries in the briefcase")]
    InsufficientApprovals,
    #[error("stated preference must not be empty")]
    EmptyPreference,
    #[error("correction instruction is shaped as a prohibition and cannot be a rewrite")]
    ProhibitionShapedCorrection,
    #[error("consolidation target is not present in the scoped briefcase")]
    ConsolidationTargetNotInBriefcase,
    #[error("consolidation has no merge or prune targets")]
    EmptyConsolidation,
    #[error("proposal kind cannot be serialized for the normal lifecycle")]
    UnsupportedLifecycleKind,
    #[error("failed to serialize proposal payload")]
    PayloadSerialize,
}
