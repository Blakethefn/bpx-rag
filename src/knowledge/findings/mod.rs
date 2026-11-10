//! Conclusions stored for one source.
//!
//! `knowledge_findings` holds conclusions for a `complete` or `partial` analysis. A reader
//! returns a finding when its version, generation, and fingerprint match that row. `incomplete`
//! omits the findings section.

pub(crate) mod reachability;
mod report;
pub(crate) mod unused;

pub use report::report;

#[cfg(test)]
mod tests;
