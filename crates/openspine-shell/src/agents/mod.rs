//! Agent implementations for the OpenSpine shell.
//!
//! Each agent is invoked once per task: the kernel spawns one shell process,
//! the shell fetches its grant view, routes to the matching agent, the agent
//! runs to completion and exits.

pub mod main_assistant;
