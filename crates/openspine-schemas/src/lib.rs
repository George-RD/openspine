//! OpenSpine core runtime schemas.
//!
//! Implements `openspec/changes/define-core-runtime-schemas/`: the versioned,
//! `deny_unknown_fields` object kinds that every other OpenSpine crate builds on.
//! This crate is pure data + pure functions (canonical JSON, digests) — no I/O.
//!
//! There is no separate JSON-Schema validation layer (decision D-028):
//! `#[serde(deny_unknown_fields)]` on every struct *is* the validation
//! engine. A `schema_version: u32` field on every top-level object records
//! which shape produced it.

pub mod action;
pub mod agent;
pub mod approval;
pub mod artifact;
pub mod audit;
pub mod briefcase;
pub mod digest;
pub mod egress;
pub mod escalation;
pub mod event;
pub mod event_bus;
pub mod grant;
pub mod grant_chain;
pub mod identity;
pub mod ids;
pub mod lineage;
pub mod model;
pub mod model_swap;
pub mod pack;
pub mod plan;
pub mod policy;
pub mod principal;
pub mod route;
pub mod selection;
pub mod task;
pub mod workflow;
