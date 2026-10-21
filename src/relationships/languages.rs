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
];

pub(crate) fn grammar_for_path(path: &Path) -> Option<Grammar> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    LANGUAGE_EXTENSIONS
        .iter()
        .find(|(extensions, _)| extensions.contains(&extension.as_str()))
        .map(|(_, grammar)| *grammar)
}
