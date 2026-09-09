// Test-first registration wrapper. Removed when the shared evaluator lands.
include!("overlay_recovery_original.rs");

#[cfg(test)]
#[path = "overlay_version_admission.rs"]
mod version_admission;
