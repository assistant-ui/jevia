//! Shared input bound: reject, never truncate or skip an oversized physical line.
use anyhow::{Context, Result, bail};
use std::io::BufRead;
pub const MAX_LINE_BYTES: usize = 8 * 1024 * 1024;

pub fn read_line(
    reader: &mut impl BufRead,
    line: &mut Vec<u8>,
    check: impl Fn() -> Result<()>,
) -> Result<bool> {
    line.clear();
    loop {
        check()?;
        let bytes = reader.fill_buf().context("could not read JSONL input")?;
        check()?;
        if bytes.is_empty() {
            // Reserve the newline an append would add to an unterminated tail.
            if line.len() == MAX_LINE_BYTES {
                bail!(
                    "unterminated JSONL line leaves no room within 8 MiB for a newline; input retained"
                );
            }
            return Ok(!line.is_empty());
        }
        let bytes = &bytes[..bytes.len().min(64 * 1024)];
        let newline = bytes.iter().position(|byte| *byte == b'\n');
        let count = newline.map_or(bytes.len(), |index| index + 1);
        if count > MAX_LINE_BYTES.saturating_sub(line.len()) {
            bail!("JSONL line exceeds 8 MiB limit (contents redacted); input retained");
        }
        line.extend_from_slice(&bytes[..count]);
        reader.consume(count);
        if newline.is_some() {
            return Ok(true);
        }
    }
}

pub fn lines(mut reader: impl BufRead) -> impl Iterator<Item = Result<String>> {
    let mut done = false;
    std::iter::from_fn(move || {
        if done {
            return None;
        }
        let mut bytes = Vec::new();
        match read_line(&mut reader, &mut bytes, || Ok(())) {
            Ok(false) => {
                done = true;
                None
            }
            Ok(true) => {
                Some(String::from_utf8(bytes).map_err(|_| {
                    anyhow::anyhow!("invalid UTF-8 in JSONL input (contents redacted)")
                }))
            }
            Err(error) => {
                done = true;
                Some(Err(error))
            }
        }
    })
}

pub fn encode(value: &impl serde::Serialize) -> Result<Vec<u8>> {
    struct Bounded(Vec<u8>);
    impl std::io::Write for Bounded {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes.len() > (MAX_LINE_BYTES - 1).saturating_sub(self.0.len()) {
                return Err(std::io::Error::other("JSONL output exceeds 8 MiB"));
            }
            self.0.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut output = Bounded(Vec::new());
    serde_json::to_writer(&mut output, value).map_err(|_| {
        anyhow::anyhow!("could not encode JSONL record within 8 MiB limit (contents redacted)")
    })?;
    let mut bytes = output.0;
    bytes.push(b'\n');
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn oversized_append_and_encoding_leave_original_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("runs.jsonl");
        let mut record = crate::tests::sample_record();
        crate::store::append(&path, &record).unwrap();
        let before = std::fs::read(&path).unwrap();
        record.task = Some("x".repeat(MAX_LINE_BYTES));
        assert!(crate::store::append(&path, &record).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert!(encode(&record).is_err());
        let mut tail = serde_json::to_vec(&crate::tests::sample_record()).unwrap();
        tail.resize(MAX_LINE_BYTES, b' ');
        std::fs::write(&path, &tail).unwrap();
        assert!(crate::store::append(&path, &crate::tests::sample_record()).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), tail);
    }
    #[test]
    fn physical_line_limit_covers_newline_eof_and_whitespace() {
        for ending in ["", "\n"] {
            let raw = " ".repeat(MAX_LINE_BYTES - 1) + ending;
            assert_eq!(
                lines(raw.as_bytes()).next().unwrap().unwrap().len(),
                MAX_LINE_BYTES - 1 + ending.len()
            );
            let raw = " ".repeat(MAX_LINE_BYTES) + "x" + ending;
            let mut lines = lines(raw.as_bytes());
            assert!(lines.next().unwrap().is_err());
            assert!(lines.next().is_none());
        }
        assert_eq!(lines("a\r\n\nb".as_bytes()).count(), 3);
    }
}
