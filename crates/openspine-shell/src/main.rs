//! `openspine-shell` — the contained per-task worker process.
//!
//! Implements `openspec/changes/implement-telegram-owner-control-slice/`
//! (4d): fetches its task grant view from the kernel and runs the selected
//! agent implementation. Every effect goes through `POST /v1/actions` on
//! the kernel — this process has no other I/O. Filled in step 4.

fn main() {
    println!("openspine-shell: not yet implemented (see Step 4 of the build plan)");
}
