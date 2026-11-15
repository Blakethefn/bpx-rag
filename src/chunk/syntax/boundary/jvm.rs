use crate::relations::Grammar;

pub(super) fn is_boundary(grammar: Grammar, kind: &str) -> bool {
    match grammar {
        Grammar::Java => matches!(
            kind,
            "class_declaration"
                | "interface_declaration"
                | "enum_declaration"
                | "record_declaration"
                | "method_declaration"
                | "constructor_declaration"
        ),
        Grammar::Dart => matches!(
            kind,
            "class_definition"
                | "enum_declaration"
                | "mixin_declaration"
                | "extension_declaration"
                | "function_declaration"
                | "method_signature"
        ),
        Grammar::Swift => matches!(
            kind,
            "function_declaration"
                | "class_declaration"
                | "struct_declaration"
                | "enum_declaration"
                | "protocol_declaration"
                | "extension_declaration"
        ),
        Grammar::Kotlin => matches!(
            kind,
            "function_declaration" | "class_declaration" | "object_declaration"
        ),
        Grammar::Scala => matches!(
            kind,
            "class_definition"
                | "trait_definition"
                | "object_definition"
                | "enum_definition"
                | "function_definition"
                | "given_definition"
                | "extension_definition"
                | "package_object"
        ),
        _ => false,
    }
}
