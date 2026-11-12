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

/// Markdown and Rust imports are resolved outside this extension list.
const RESOLVED_ELSEWHERE: &[Grammar] = &[Grammar::Markdown, Grammar::Rust];

/// Grammars that share one import candidate list. Earlier rows are tried first.
const RESOLUTION_GROUPS: &[&[Grammar]] =
    &[&[Grammar::TypeScript, Grammar::Tsx, Grammar::JavaScript]];

/// Extra suffixes for import resolution.
/// These do not change which grammar a path is detected as.
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

pub(crate) fn candidate_extensions(grammar: Grammar) -> Vec<&'static str> {
    if RESOLVED_ELSEWHERE.contains(&grammar) {
        return Vec::new();
    }
    let mut extensions = Vec::new();
    for_resolution_group(grammar, |member| {
        for extension in extensions_for(member) {
            push_unique(&mut extensions, extension);
        }
    });
    for_resolution_group(grammar, |member| {
        for (candidate, extras) in RESOLUTION_EXTRAS {
            if *candidate == member {
                for extension in *extras {
                    push_unique(&mut extensions, extension);
                }
            }
        }
    });
    extensions
}

pub(crate) fn is_resolution_extra(grammar: Grammar, extension: &str) -> bool {
    if RESOLVED_ELSEWHERE.contains(&grammar) {
        return false;
    }
    let mut detected = false;
    for_resolution_group(grammar, |member| {
        if extensions_for(member).contains(&extension) {
            detected = true;
        }
    });
    if detected {
        return false;
    }
    let mut extra = false;
    for_resolution_group(grammar, |member| {
        if RESOLUTION_EXTRAS
            .iter()
            .any(|(candidate, extras)| *candidate == member && extras.contains(&extension))
        {
            extra = true;
        }
    });
    extra
}

fn for_resolution_group(grammar: Grammar, mut visit: impl FnMut(Grammar)) {
    if let Some(group) = RESOLUTION_GROUPS
        .iter()
        .find(|group| group.contains(&grammar))
    {
        for member in *group {
            visit(*member);
        }
    } else {
        visit(grammar);
    }
}

fn push_unique<'a>(extensions: &mut Vec<&'a str>, extension: &'a str) {
    if !extensions.contains(&extension) {
        extensions.push(extension);
    }
}

#[cfg(test)]
mod tests;
