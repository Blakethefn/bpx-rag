use tree_sitter::Node;

pub(in crate::chunk) fn include_leading_trivia(node: Node<'_>, content: &str) -> usize {
    let mut start = node.start_byte();
    let mut previous = node.prev_named_sibling();
    while let Some(sibling) = previous {
        if !is_leading_trivia(sibling.kind())
            || sibling.end_byte() > start
            || !starts_on_own_line(content, sibling.start_byte())
        {
            break;
        }
        let Some(gap) = content.get(sibling.end_byte()..start) else {
            break;
        };
        if !gap.chars().all(char::is_whitespace) {
            break;
        }
        start = sibling.start_byte();
        previous = sibling.prev_named_sibling();
    }
    start
}

fn starts_on_own_line(content: &str, byte: usize) -> bool {
    let Some(prefix) = content.get(..byte) else {
        return false;
    };
    let line_start = prefix.rfind('\n').map_or(0, |newline| newline + 1);
    content
        .get(line_start..byte)
        .is_some_and(|indentation| indentation.chars().all(char::is_whitespace))
}

fn is_leading_trivia(kind: &str) -> bool {
    kind.contains("comment")
        || kind.contains("attribute")
        || matches!(kind, "decorator" | "shebang")
}
