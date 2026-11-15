use std::path::Path;

mod cfamily;
mod modules;
mod scripting;

struct LanguageDefinition {
    names: &'static [&'static str],
    extensions: &'static [&'static str],
}

const LANGUAGE_DEFINITIONS: &[&[LanguageDefinition]] = &[
    cfamily::DEFINITIONS,
    scripting::DEFINITIONS,
    modules::DEFINITIONS,
];

fn definition(name: &str) -> Option<&'static LanguageDefinition> {
    LANGUAGE_DEFINITIONS
        .iter()
        .copied()
        .flatten()
        .find(|definition| {
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
