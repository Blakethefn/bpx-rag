use anyhow::{Context, Result};
use std::io::{BufRead, BufReader, Read, Write};

pub fn read_message(reader: &mut BufReader<impl Read>) -> Result<Option<String>> {
    loop {
        let mut line = String::new();
        let bytes_read = reader.read_line(&mut line).context("Failed to read line")?;

        if bytes_read == 0 {
            return Ok(None);
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if trimmed.starts_with('{') {
            return Ok(Some(trimmed.to_string()));
        }

        if let Some(len_str) = trimmed.strip_prefix("Content-Length:") {
            let length: usize = len_str
                .trim()
                .parse()
                .context("Invalid Content-Length value")?;

            loop {
                let mut header = String::new();
                reader
                    .read_line(&mut header)
                    .context("Failed to read header")?;
                if header.trim().is_empty() {
                    break;
                }
            }

            let mut body = vec![0u8; length];
            reader
                .read_exact(&mut body)
                .context("Failed to read message body")?;

            let message = String::from_utf8(body).context("Message body is not valid UTF-8")?;

            return Ok(Some(message));
        }
    }
}

pub fn write_message(writer: &mut impl Write, body: &str) -> Result<()> {
    writer.write_all(body.as_bytes())?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests;
