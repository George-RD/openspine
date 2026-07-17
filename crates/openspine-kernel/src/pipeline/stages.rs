//! Pipeline stage taxonomy (AD-090 / pipeline sequence).
//!
//! The canonical nine-stage sequence is declared once here; the driver's
//! synchronous prefix is derived from it and pinned by unit tests.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipelineStage {
    Event,
    Verify,
    Identify,
    Route,
    Compose,
    Grant,
    Run,
    Gate,
    Audit,
}

impl PipelineStage {
    /// The canonical, complete stage sequence — declared in exactly one
    /// place. Tests pin this order.
    pub const SEQUENCE: [PipelineStage; 9] = [
        PipelineStage::Event,
        PipelineStage::Verify,
        PipelineStage::Identify,
        PipelineStage::Route,
        PipelineStage::Compose,
        PipelineStage::Grant,
        PipelineStage::Run,
        PipelineStage::Gate,
        PipelineStage::Audit,
    ];

    /// The synchronous prefix the driver actually executes: derived
    /// element-by-element from [`Self::SEQUENCE`], truncated before `Gate`,
    /// so the two declarations cannot drift. The driver's executed-stage
    /// trace is pinned to this prefix by the unit tests.
    pub const SYNC_PREFIX: [PipelineStage; 7] = [
        Self::SEQUENCE[0],
        Self::SEQUENCE[1],
        Self::SEQUENCE[2],
        Self::SEQUENCE[3],
        Self::SEQUENCE[4],
        Self::SEQUENCE[5],
        Self::SEQUENCE[6],
    ];
}
const _: () = {
    assert!(matches!(PipelineStage::SEQUENCE[7], PipelineStage::Gate));
    assert!(matches!(PipelineStage::SEQUENCE[8], PipelineStage::Audit));
    assert!(PipelineStage::SYNC_PREFIX.len() + 2 == PipelineStage::SEQUENCE.len());
};
