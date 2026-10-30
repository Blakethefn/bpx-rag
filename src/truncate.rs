/// Generate a preview (first N bytes) of content
pub fn preview(content: &str, max_bytes: usize) -> String {
    if content.len() <= max_bytes {
        return content.to_string();
    }
    let truncated = safe_truncate(content, max_bytes);
    format!(
        "{truncated}\n\n... [truncated, {total} bytes total]",
        total = content.len()
    )
}

/// Truncate a string at a UTF-8 safe boundary from the start
fn safe_truncate(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    // Find the last newline before max_bytes for cleaner cuts
    let slice = &s[..max_bytes.min(s.len())];
    match slice.rfind('\n') {
        Some(pos) if pos > max_bytes / 2 => &s[..pos],
        _ => {
            // Fall back to char boundary
            let mut end = max_bytes.min(s.len());
            while end > 0 && !s.is_char_boundary(end) {
                end -= 1;
            }
            &s[..end]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;



    #[test]
    fn test_preview() {
        let content = "a".repeat(500);
        let result = preview(&content, 100);
        assert!(result.contains("truncated"));
        assert!(result.contains("500 bytes total"));
    }
}
