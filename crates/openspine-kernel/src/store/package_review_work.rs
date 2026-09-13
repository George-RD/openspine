impl Store {
    pub(crate) fn package_outstanding_work_with_reviews(
        &self,
        as_of: Timestamp,
        _artifacts: &crate::artifact_store::ArtifactStore,
    ) -> Result<PackageOutstandingWork, StoreError> {
        // Characterization seam: the next change supplies canonical review
        // evidence. Until then this retains the conservative existing census.
        self.package_outstanding_work(as_of)
    }
}
