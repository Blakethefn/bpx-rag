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

fn safe_truncate(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let slice = &s[..max_bytes.min(s.len())];
    match slice.rfind('\n') {
        Some(pos) if pos > max_bytes / 2 => &s[..pos],
        _ => {
            let mut end = max_bytes.min(s.len());
            while end > 0 && !s.is_char_boundary(end) {
                end -= 1;
            }
            &s[..end]
        }
    }
}

#[cfg(test)]
mod tests;
