use std::path::Path;

struct LanguageDefinition {
    names: &'static [&'static str],
    extensions: &'static [&'static str],
}

const LANGUAGE_DEFINITIONS: &[LanguageDefinition] = &[
    LanguageDefinition {
        names: &["rust", "rs"],
        extensions: &["rs"],
    },
    LanguageDefinition {
        names: &["javascript", "js", "jsx"],
        extensions: &["js", "jsx", "mjs", "cjs"],
    },
    LanguageDefinition {
        names: &["typescript", "ts", "tsx"],
        extensions: &["ts", "tsx", "mts", "cts"],
    },
    LanguageDefinition {
        names: &["c"],
        extensions: &["c", "h"],
    },
    LanguageDefinition {
        names: &["cpp", "c++", "cxx"],
        extensions: &["cc", "hh", "cpp", "hpp", "cxx", "hxx", "h"],
    },
    LanguageDefinition {
        names: &["csharp", "c#", "cs"],
        extensions: &["cs"],
    },
    LanguageDefinition {
        names: &["go", "golang"],
        extensions: &["go"],
    },
    LanguageDefinition {
        names: &["java"],
        extensions: &["java"],
    },
    LanguageDefinition {
        names: &["python", "py"],
        extensions: &["py", "pyi", "pyw"],
    },
    LanguageDefinition {
        names: &["php"],
        extensions: &["php", "phtml"],
    },
    LanguageDefinition {
        names: &["ruby", "rb"],
        extensions: &["rb", "rake"],
    },
    LanguageDefinition {
        names: &["shell", "bash", "sh"],
        extensions: &["sh", "bash"],
    },
    LanguageDefinition {
        names: &["lua"],
        extensions: &["lua"],
    },
    LanguageDefinition {
        names: &["dart"],
        extensions: &["dart"],
    },
    LanguageDefinition {
        names: &["swift"],
        extensions: &["swift"],
    },
    LanguageDefinition {
        names: &["kotlin", "kt"],
        extensions: &["kt", "kts"],
    },
    LanguageDefinition {
        names: &["scala"],
        extensions: &["scala", "sc"],
    },
    LanguageDefinition {
        names: &["sql"],
        extensions: &["sql"],
    },
    LanguageDefinition {
        names: &["r"],
        extensions: &["r"],
    },
    LanguageDefinition {
        names: &["julia"],
        extensions: &["jl"],
    },
    LanguageDefinition {
        names: &["elixir"],
        extensions: &["ex", "exs"],
    },
    LanguageDefinition {
        names: &["erlang"],
        extensions: &["erl", "hrl"],
    },
    LanguageDefinition {
        names: &["fsharp", "f#"],
        extensions: &["fs", "fsi", "fsx"],
    },
    LanguageDefinition {
        names: &["haskell"],
        extensions: &["hs", "lhs"],
    },
    LanguageDefinition {
        names: &["clojure"],
        extensions: &["clj", "cljs", "cljc", "edn"],
    },
    LanguageDefinition {
        names: &["perl"],
        extensions: &["pl", "pm", "t"],
    },
    LanguageDefinition {
        names: &["powershell", "pwsh", "ps1"],
        extensions: &["ps1", "psm1", "psd1"],
    },
    LanguageDefinition {
        names: &["objective-c", "objc"],
        extensions: &["m", "mm"],
    },
    LanguageDefinition {
        names: &["ocaml"],
        extensions: &["ml", "mli"],
    },
    LanguageDefinition {
        names: &["groovy"],
        extensions: &["groovy", "gvy", "gy", "gsh"],
    },
    LanguageDefinition {
        names: &["fortran"],
        extensions: &["f", "for", "f90", "f95", "f03", "f08"],
    },
    LanguageDefinition {
        names: &["cobol"],
        extensions: &["cob", "cbl"],
    },
    LanguageDefinition {
        names: &["solidity"],
        extensions: &["sol"],
    },
    LanguageDefinition {
        names: &["zig"],
        extensions: &["zig"],
    },
    LanguageDefinition {
        names: &["nim"],
        extensions: &["nim", "nims"],
    },
    LanguageDefinition {
        names: &["crystal"],
        extensions: &["cr"],
    },
    LanguageDefinition {
        names: &["html"],
        extensions: &["html", "htm"],
    },
    LanguageDefinition {
        names: &["css"],
        extensions: &["css"],
    },
    LanguageDefinition {
        names: &["scss"],
        extensions: &["scss", "sass"],
    },
    LanguageDefinition {
        names: &["less"],
        extensions: &["less"],
    },
    LanguageDefinition {
        names: &["vue"],
        extensions: &["vue"],
    },
    LanguageDefinition {
        names: &["svelte"],
        extensions: &["svelte"],
    },
    LanguageDefinition {
        names: &["markdown", "md"],
        extensions: &["md", "markdown", "mdx"],
    },
    LanguageDefinition {
        names: &["json"],
        extensions: &["json", "jsonc"],
    },
    LanguageDefinition {
        names: &["yaml", "yml"],
        extensions: &["yaml", "yml"],
    },
    LanguageDefinition {
        names: &["toml"],
        extensions: &["toml"],
    },
    LanguageDefinition {
        names: &["xml"],
        extensions: &["xml", "xsd", "xsl", "xslt"],
    },
    LanguageDefinition {
        names: &["graphql", "gql"],
        extensions: &["graphql", "gql"],
    },
    LanguageDefinition {
        names: &["protobuf", "proto"],
        extensions: &["proto"],
    },
    LanguageDefinition {
        names: &["make", "makefile", "gnumakefile"],
        extensions: &["mk"],
    },
];

fn definition(name: &str) -> Option<&'static LanguageDefinition> {
    LANGUAGE_DEFINITIONS.iter().find(|definition| {
        definition
            .names
            .iter()
            .any(|alias| alias.eq_ignore_ascii_case(name))
    })
}

pub(crate) fn extensions_for_name(name: &str) -> Vec<String> {
    match definition(name) {
        Some(definition) => definition
            .extensions
            .iter()
            .map(|extension| (*extension).to_string())
            .collect(),
        None if is_safe_extension(name) => vec![name.to_ascii_lowercase()],
        None => Vec::new(),
    }
}

pub(crate) fn aliases_for_name(name: &str) -> Option<&'static [&'static str]> {
    definition(name).map(|definition| definition.names)
}

pub(crate) fn file_names_for_name(name: &str) -> &'static [&'static str] {
    match definition(name).and_then(|definition| definition.names.first().copied()) {
        Some("make") => &["makefile", "gnumakefile"],
        _ => &[],
    }
}

fn is_safe_extension(extension: &str) -> bool {
    !extension.is_empty()
        && extension
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '-')
}

pub(crate) fn matches_language(expected: &str, path: &Path, frontmatter: Option<&str>) -> bool {
    let expected_definition = definition(expected);
    if let Some(frontmatter_language) = frontmatter {
        let matches_frontmatter = match expected_definition {
            Some(expected) => definition(frontmatter_language)
                .is_some_and(|actual| std::ptr::eq(expected, actual)),
            None => expected.eq_ignore_ascii_case(frontmatter_language),
        };
        if matches_frontmatter {
            return true;
        }
    }

    if file_names_for_name(expected).iter().any(|file_name| {
        path.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case(file_name))
    }) {
        return true;
    }

    let Some(extension) = path.extension().and_then(|extension| extension.to_str()) else {
        return false;
    };
    match expected_definition {
        Some(expected) => expected
            .extensions
            .iter()
            .any(|candidate| candidate.eq_ignore_ascii_case(extension)),
        None => is_safe_extension(expected) && extension.eq_ignore_ascii_case(expected),
    }
}
