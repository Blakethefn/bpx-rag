use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Grammar {
    Markdown,
    Rust,
    JavaScript,
    TypeScript,
    Tsx,
    C,
    Cpp,
    CSharp,
    Go,
    Java,
    Python,
    Php,
    Ruby,
    Bash,
    Lua,
    Dart,
    Swift,
    Kotlin,
    Sql,
    Scala,
    Haskell,
    Elixir,
    Erlang,
    R,
    Julia,
    Perl,
    PowerShell,
    Solidity,
    Html,
    Css,
}

const LANGUAGE_EXTENSIONS: &[(&[&str], Grammar)] = &[
    (&["md"], Grammar::Markdown),
    (&["rs"], Grammar::Rust),
    (&["js", "jsx", "mjs", "cjs"], Grammar::JavaScript),
    (&["ts"], Grammar::TypeScript),
    (&["tsx"], Grammar::Tsx),
    (&["c", "h"], Grammar::C),
    (&["cc", "hh", "cpp", "hpp", "cxx", "hxx"], Grammar::Cpp),
    (&["cs"], Grammar::CSharp),
    (&["go"], Grammar::Go),
    (&["java"], Grammar::Java),
    (&["py", "pyi"], Grammar::Python),
    (&["php", "phtml"], Grammar::Php),
    (&["rb", "rake"], Grammar::Ruby),
    (&["sh", "bash"], Grammar::Bash),
    (&["lua"], Grammar::Lua),
    (&["dart"], Grammar::Dart),
    (&["swift"], Grammar::Swift),
    (&["kt", "kts"], Grammar::Kotlin),
    (&["sql"], Grammar::Sql),
    (&["scala", "sc"], Grammar::Scala),
    (&["hs", "lhs"], Grammar::Haskell),
    (&["ex", "exs"], Grammar::Elixir),
    (&["erl", "hrl"], Grammar::Erlang),
    (&["r"], Grammar::R),
    (&["jl"], Grammar::Julia),
    (&["pl", "pm", "t"], Grammar::Perl),
    (&["ps1", "psm1", "psd1"], Grammar::PowerShell),
    (&["sol"], Grammar::Solidity),
    (&["html", "htm"], Grammar::Html),
    (&["css"], Grammar::Css),
];

const RESOLVED_ELSEWHERE: &[Grammar] = &[Grammar::Markdown, Grammar::Rust];

const RESOLUTION_GROUPS: &[&[Grammar]] =
    &[&[Grammar::TypeScript, Grammar::Tsx, Grammar::JavaScript]];

const RESOLUTION_EXTRAS: &[(Grammar, &[&str])] = &[
    (Grammar::Cpp, &["h"]),
    (Grammar::JavaScript, &["json"]),
    (Grammar::TypeScript, &["json"]),
    (Grammar::Tsx, &["json"]),
    (Grammar::R, &["R"]),
    (
        Grammar::Html,
        &["js", "jsx", "ts", "tsx", "css", "json", "svg"],
    ),
    (Grammar::Css, &["scss", "sass", "less"]),
];

pub(crate) fn grammar_for_path(path: &Path) -> Option<Grammar> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    LANGUAGE_EXTENSIONS
        .iter()
        .find(|(extensions, _)| extensions.contains(&extension.as_str()))
        .map(|(_, grammar)| *grammar)
}

pub(crate) fn extensions_for(grammar: Grammar) -> &'static [&'static str] {
    LANGUAGE_EXTENSIONS
        .iter()
        .find(|(_, candidate)| *candidate == grammar)
        .map(|(extensions, _)| *extensions)
        .unwrap_or(&[])
}

mod candidates;

pub(crate) use candidates::{candidate_extensions, is_resolution_extra};

#[cfg(test)]
mod tests;
