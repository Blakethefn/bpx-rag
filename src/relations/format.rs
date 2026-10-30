use std::collections::{BTreeMap, BTreeSet};

use super::types::{FileAnalysis, FileRelationship};

pub fn format_analysis(analysis: &FileAnalysis) -> String {
    let mut output = format!(
        "File relationships for `{}`\nRoot: `{}`\nScanned {} supported files (no database used).\n\n",
        analysis.file, analysis.root, analysis.scanned_files
    );
    append_relationship_section(&mut output, "Outgoing", &analysis.outgoing);
    output.push('\n');
    append_relationship_section(&mut output, "Incoming", &analysis.incoming);
    if !analysis.warnings.is_empty() {
        output.push_str("\nWarnings:\n");
        for warning in &analysis.warnings {
            output.push_str(&format!("- {warning}\n"));
        }
    }
    output
}

fn append_relationship_section(
    output: &mut String,
    title: &str,
    relationships: &[FileRelationship],
) {
    let mut grouped =
        BTreeMap::<(String, Option<String>, String, String), Vec<&FileRelationship>>::new();
    for relationship in relationships {
        let unresolved_specifier = if relationship.to.is_none() {
            relationship.specifier.clone()
        } else {
            String::new()
        };
        grouped
            .entry((
                relationship.from.clone(),
                relationship.to.clone(),
                relationship.kind.clone(),
                unresolved_specifier,
            ))
            .or_default()
            .push(relationship);
    }

    output.push_str(&format!("{title} ({}):\n", grouped.len()));
    if grouped.is_empty() {
        output.push_str("- None found.\n");
        return;
    }
    for ((from, to, kind, _), evidence) in grouped {
        let destination = to.as_deref().unwrap_or("unresolved local reference");
        let lines = evidence
            .iter()
            .map(|relationship| relationship.line)
            .collect::<BTreeSet<_>>();
        let specifiers = evidence
            .iter()
            .map(|relationship| relationship.specifier.as_str())
            .collect::<BTreeSet<_>>();
        let line_label = if lines.len() == 1 { "line" } else { "lines" };
        let specifier_label = if specifiers.len() == 1 { "ref" } else { "refs" };
        output.push_str(&format!(
            "- `{from}` -> `{destination}` ({kind}, {line_label} {}, {specifier_label} {})\n",
            lines
                .iter()
                .map(usize::to_string)
                .collect::<Vec<_>>()
                .join(", "),
            specifiers
                .iter()
                .map(|specifier| format!("`{specifier}`"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
}
