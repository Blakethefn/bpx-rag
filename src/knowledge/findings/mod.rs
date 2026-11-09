//! Conclusions stored for one source.
//!
//! `knowledge_findings` holds only conclusions, and only when the analysis is complete. A reader
//! returns a finding when its version, generation, and fingerprint match that row.

pub(crate) mod reachability;
mod report;
pub(crate) mod unused;

pub use report::report;

#[cfg(test)]
mod tests;
