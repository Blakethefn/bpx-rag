use std::collections::HashSet;

pub(crate) fn split_identifier(identifier: &str) -> Vec<String> {
    let chars: Vec<char> = identifier.chars().collect();
    let mut words = Vec::new();
    let mut current = String::new();
    for (index, &character) in chars.iter().enumerate() {
        if character == '_' || character == '-' {
            push_word(&mut words, &mut current);
            continue;
        }
        if index > 0 && starts_word(&chars, index) {
            push_word(&mut words, &mut current);
        }
        current.extend(character.to_lowercase());
    }
    push_word(&mut words, &mut current);
    words
}

fn starts_word(chars: &[char], index: usize) -> bool {
    let previous = chars[index - 1];
    let current = chars[index];
    if (previous.is_lowercase() || previous.is_ascii_digit()) && current.is_uppercase() {
        return true;
    }
    previous.is_uppercase()
        && current.is_uppercase()
        && chars.get(index + 1).is_some_and(|next| next.is_lowercase())
}

fn push_word(words: &mut Vec<String>, current: &mut String) {
    if !current.is_empty() {
        words.push(std::mem::take(current));
    }
}

pub(crate) fn identifier_words(text: &str) -> String {
    let mut seen = HashSet::new();
    let mut phrases = Vec::new();
    for token in text.split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '-')) {
        let identifier = token.trim_matches(|c| c == '_' || c == '-');
        if !identifier.starts_with(char::is_alphabetic) || !seen.insert(identifier) {
            continue;
        }
        let words = split_identifier(identifier);
        if words.len() > 1 {
            phrases.push(words.join(" "));
        }
    }
    phrases.join(" ")
}
