#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum LexMode {
    Rust,
    Script,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum TokenKind {
    Ident,
    String,
    Punctuation,
}

#[derive(Clone)]
pub(super) struct Token {
    pub(super) value: String,
    pub(super) line: usize,
    pub(super) kind: TokenKind,
}

pub(super) fn lex_code(source: &str, mode: LexMode) -> Vec<Token> {
    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0usize;
    let mut line = 1usize;
    while index < bytes.len() {
        if bytes[index].is_ascii_whitespace() {
            if bytes[index] == b'\n' {
                line += 1;
            }
            index += 1;
            continue;
        }
        if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'/') {
            index += 2;
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
            continue;
        }
        if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'*') {
            index += 2;
            let mut depth = 1usize;
            while index < bytes.len() && depth > 0 {
                if bytes[index] == b'\n' {
                    line += 1;
                    index += 1;
                } else if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'*') {
                    depth += 1;
                    index += 2;
                } else if bytes[index] == b'*' && bytes.get(index + 1) == Some(&b'/') {
                    depth -= 1;
                    index += 2;
                } else {
                    index += 1;
                }
            }
            continue;
        }

        let quote = bytes[index];
        if quote == b'"' || (mode == LexMode::Script && (quote == b'\'' || quote == b'`')) {
            let token_line = line;
            index += 1;
            let mut value = String::new();
            while index < bytes.len() && bytes[index] != quote {
                if bytes[index] == b'\\' && index + 1 < bytes.len() {
                    value.push(bytes[index + 1] as char);
                    index += 2;
                } else {
                    if bytes[index] == b'\n' {
                        line += 1;
                    }
                    value.push(bytes[index] as char);
                    index += 1;
                }
            }
            if index < bytes.len() {
                index += 1;
            }
            tokens.push(Token {
                value,
                line: token_line,
                kind: TokenKind::String,
            });
            continue;
        }
        if mode == LexMode::Rust && quote == b'\'' {
            tokens.push(Token {
                value: "'".to_string(),
                line,
                kind: TokenKind::Punctuation,
            });
            index += 1;
            continue;
        }
        if is_ident_start(bytes[index]) {
            let start = index;
            index += 1;
            while index < bytes.len() && is_ident_continue(bytes[index]) {
                index += 1;
            }
            tokens.push(Token {
                value: String::from_utf8_lossy(&bytes[start..index]).to_string(),
                line,
                kind: TokenKind::Ident,
            });
            continue;
        }
        if bytes[index] == b':' && bytes.get(index + 1) == Some(&b':') {
            tokens.push(Token {
                value: "::".to_string(),
                line,
                kind: TokenKind::Punctuation,
            });
            index += 2;
            continue;
        }
        tokens.push(Token {
            value: (bytes[index] as char).to_string(),
            line,
            kind: TokenKind::Punctuation,
        });
        index += 1;
    }
    tokens
}

fn is_ident_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_' || byte == b'$'
}

fn is_ident_continue(byte: u8) -> bool {
    is_ident_start(byte) || byte.is_ascii_digit()
}
