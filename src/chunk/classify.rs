#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ContentType {
    Markdown,
    Code,
    PlainText,
}

pub(super) fn classify_content(content: &str) -> ContentType {
    let has_php_tag = content.contains("<?php") || content.contains("<?=");
    let has_script_pair = content.contains("<script") && content.contains("</script>");
    let has_style_pair = content.contains("<style") && content.contains("</style>");

    let mut md_score: u32 = 0;
    let mut md_code_counter: u32 = 0;
    let mut line_count: u32 = 0;

    let mut non_empty_lines: u32 = 0;
    let mut code_endings: u32 = 0;
    let mut has_func = false;

    for line in content.lines() {
        line_count += 1;
        let trimmed = line.trim();

        if trimmed.starts_with("# ")
            || trimmed.starts_with("## ")
            || trimmed.starts_with("### ")
            || trimmed.starts_with("#### ")
        {
            md_score += 1;
        }
        if trimmed.starts_with("```") {
            md_score += 1;
        }
        if (trimmed.starts_with("- ") || trimmed.starts_with("* "))
            && !trimmed.contains('{')
            && !trimmed.contains(';')
        {
            md_score += 1;
        }

        if trimmed.contains("<?php")
            || trimmed.starts_with("<script")
            || trimmed.starts_with("<style")
            || trimmed.starts_with("<html")
            || trimmed.starts_with("<!DOCTYPE")
            || trimmed.starts_with("import ")
            || trimmed.starts_with("from ")
            || trimmed.starts_with("package ")
            || trimmed.starts_with("use ")
        {
            md_code_counter += 1;
        }
        if trimmed.ends_with(';') || trimmed.ends_with('{') {
            md_code_counter += 1;
        }

        if !trimmed.is_empty() {
            non_empty_lines += 1;
            if trimmed.ends_with(';') || trimmed.ends_with('{') || trimmed.ends_with('}') {
                code_endings += 1;
            }
            if !has_func {
                has_func = trimmed.starts_with("function ")
                    || trimmed.starts_with("def ")
                    || trimmed.starts_with("fn ")
                    || trimmed.starts_with("pub fn ")
                    || trimmed.contains("function(")
                    || trimmed.contains("function (")
                    || trimmed.starts_with("class ");
            }
        }
    }

    if line_count > 0 && md_score > 0 {
        let md_passes =
            (md_score as f64 / line_count as f64) > 0.02 && md_code_counter <= md_score * 2;
        if md_passes {
            return ContentType::Markdown;
        }
    }

    if has_php_tag || has_script_pair || has_style_pair {
        return ContentType::Code;
    }
    if non_empty_lines > 0 {
        let code_ratio = code_endings as f64 / non_empty_lines as f64;
        if code_ratio > 0.4 || (has_func && code_endings > 5) {
            return ContentType::Code;
        }
    }

    ContentType::PlainText
}
