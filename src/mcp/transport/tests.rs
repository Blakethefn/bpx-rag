use super::*;
use std::io::Cursor;

#[test]
fn test_newline_delimited_roundtrip() {
    let message = r#"{"jsonrpc":"2.0","id":1,"method":"test"}"#;

    // Write (newline-delimited)
    let mut buf = Vec::new();
    write_message(&mut buf, message).unwrap();

    // Read back
    let mut reader = BufReader::new(Cursor::new(buf));
    let result = read_message(&mut reader).unwrap().unwrap();
    assert_eq!(result, message);
}

#[test]
fn test_content_length_framing() {
    let message = r#"{"jsonrpc":"2.0","id":1,"method":"test"}"#;
    let framed = format!("Content-Length: {}\r\n\r\n{}", message.len(), message);

    let mut reader = BufReader::new(Cursor::new(framed.into_bytes()));
    let result = read_message(&mut reader).unwrap().unwrap();
    assert_eq!(result, message);
}

#[test]
fn test_eof_returns_none() {
    let mut reader = BufReader::new(Cursor::new(Vec::<u8>::new()));
    let result = read_message(&mut reader).unwrap();
    assert!(result.is_none());
}

#[test]
fn test_blank_lines_skipped() {
    let input = "\n\n{\"jsonrpc\":\"2.0\",\"id\":1}\n";
    let mut reader = BufReader::new(Cursor::new(input.as_bytes().to_vec()));
    let result = read_message(&mut reader).unwrap().unwrap();
    assert_eq!(result, r#"{"jsonrpc":"2.0","id":1}"#);
}
