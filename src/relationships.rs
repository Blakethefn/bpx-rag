mod format;
mod languages;
mod markdown;
mod metadata;
mod parsers;
mod paths;
mod polyglot;
mod rust;
mod scan;
mod script;
mod types;

pub use format::format_analysis;
pub use scan::analyze_file;

#[cfg(test)]
mod tests;
