mod format;
mod lexer;
mod markdown;
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
