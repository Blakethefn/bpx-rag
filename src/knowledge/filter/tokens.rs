use anyhow::{Result, anyhow};

pub(super) fn split(filter: &str) -> Result<Vec<String>> {
    let mut tokens = Vec::new();
    let mut token = String::new();
    let mut quoted = false;
    let mut chars = filter.chars().peekable();

    while let Some(character) = chars.next() {
        match character {
            '"' => quoted = !quoted,
            '\\' if quoted && chars.peek() == Some(&'"') => token.push(chars.next().unwrap()),
            whitespace if whitespace.is_whitespace() && !quoted => {
                if !token.is_empty() {
                    tokens.push(std::mem::take(&mut token));
                }
            }
            other => token.push(other),
        }
    }

    if quoted {
        return Err(anyhow!("filter contains an unterminated quoted value"));
    }
    if !token.is_empty() {
        tokens.push(token);
    }
    Ok(tokens)
}
