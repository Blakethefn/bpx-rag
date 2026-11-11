use tree_sitter::Node;

pub(super) fn rust_use_paths(argument: Node<'_>, content: &str) -> Vec<Vec<String>> {
    fn path_segments(node: Node<'_>, content: &str) -> Vec<String> {
        node.utf8_text(content.as_bytes())
            .unwrap_or_default()
            .split("::")
            .map(str::trim)
            .filter(|segment| !segment.is_empty() && *segment != "*")
            .map(|segment| segment.strip_prefix("r#").unwrap_or(segment).to_string())
            .collect()
    }

    fn collect(node: Node<'_>, prefix: &[String], content: &str, output: &mut Vec<Vec<String>>) {
        match node.kind() {
            "scoped_use_list" => {
                let mut current = prefix.to_vec();
                if let Some(path) = node.child_by_field_name("path") {
                    current.extend(path_segments(path, content));
                }
                if let Some(list) = node.child_by_field_name("list") {
                    collect(list, &current, content, output);
                }
            }
            "use_list" => {
                for index in 0..node.named_child_count() {
                    if let Some(child) = node.named_child(index as u32) {
                        collect(child, prefix, content, output);
                    }
                }
            }
            "use_as_clause" => {
                if let Some(path) = node.child_by_field_name("path") {
                    let mut current = prefix.to_vec();
                    current.extend(path_segments(path, content));
                    if !current.is_empty() {
                        output.push(current);
                    }
                }
            }
            "use_wildcard" => {
                let mut current = prefix.to_vec();
                if let Some(path) = node.named_child(0) {
                    current.extend(path_segments(path, content));
                }
                if !current.is_empty() {
                    output.push(current);
                }
            }
            _ => {
                let mut current = prefix.to_vec();
                current.extend(path_segments(node, content));
                if !current.is_empty() {
                    output.push(current);
                }
            }
        }
    }

    let mut output = Vec::new();
    collect(argument, &[], content, &mut output);
    output
}
