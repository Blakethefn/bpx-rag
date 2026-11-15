pub(super) const DEFINITIONS: &[super::LanguageDefinition] = &[
    super::LanguageDefinition {
        names: &["rust", "rs"],
        extensions: &["rs"],
    },
    super::LanguageDefinition {
        names: &["c"],
        extensions: &["c", "h"],
    },
    super::LanguageDefinition {
        names: &["cpp", "c++", "cxx"],
        extensions: &["cc", "hh", "cpp", "hpp", "cxx", "hxx", "h"],
    },
    super::LanguageDefinition {
        names: &["csharp", "c#", "cs"],
        extensions: &["cs"],
    },
    super::LanguageDefinition {
        names: &["go", "golang"],
        extensions: &["go"],
    },
    super::LanguageDefinition {
        names: &["objective-c", "objc"],
        extensions: &["m", "mm"],
    },
    super::LanguageDefinition {
        names: &["zig"],
        extensions: &["zig"],
    },
    super::LanguageDefinition {
        names: &["fortran"],
        extensions: &["f", "for", "f90", "f95", "f03", "f08"],
    },
    super::LanguageDefinition {
        names: &["cobol"],
        extensions: &["cob", "cbl"],
    },
    super::LanguageDefinition {
        names: &["nim"],
        extensions: &["nim", "nims"],
    },
    super::LanguageDefinition {
        names: &["crystal"],
        extensions: &["cr"],
    },
];
