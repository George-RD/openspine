//! Kernel boundary for per-worker briefcase projections.
use openspine_schemas::briefcase::{BriefcaseView, WorkerVisibility};
use ulid::Ulid;

use crate::briefcase::BriefcaseKernelError;

#[allow(dead_code)]
pub fn view_for_worker(
    store: &crate::store::Store,
    task_grant_id: Ulid,
    worker_id: Ulid,
) -> Result<BriefcaseView, BriefcaseKernelError> {
    let briefcase = store
        .find_briefcase(task_grant_id)?
        .ok_or_else(|| BriefcaseKernelError::SourceUnavailable("briefcase".into()))?;
    let visibility = match store.worker_visibility(task_grant_id, worker_id)? {
        Some(record) => record,
        None => {
            let record = WorkerVisibility::worker_default(worker_id);
            store.set_worker_visibility(task_grant_id, &record)?;
            record
        }
    };
    Ok(briefcase.view_for(&visibility))
}
