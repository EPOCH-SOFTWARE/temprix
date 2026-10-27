//! TEMPRIX as a library. The `temprix` binary (src/main.rs) is a thin CLI
//! and daemon loop over these modules.

pub mod clone;
pub mod config;
pub mod daemon;
pub mod exclude;
pub mod finder;
pub mod history;
pub mod markers;
pub mod resolve;
