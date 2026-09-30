use super::*;
use std::time::Instant;

pub(super) fn check_deadline(deadline: Option<Instant>) -> Result<()> {
    if deadline.is_some_and(|end| Instant::now() >= end) {
        bail!("history work budget exhausted; journal retained for retry");
    }
    Ok(())
}

/// Reuse one line buffer. Check between chunks too, not only at newlines.
/// Blocking OS I/O and decoding one record are not preemptible.
pub(super) fn read_line_until(
    reader: &mut impl BufRead,
    line: &mut Vec<u8>,
    deadline: Option<Instant>,
) -> Result<bool> {
    line.clear();
    if deadline.is_none() {
        return reader
            .read_until(b'\n', line)
            .map(|count| count != 0)
            .context("could not read run history");
    }
    loop {
        check_deadline(deadline)?;
        let bytes = reader.fill_buf().context("could not read run history")?;
        check_deadline(deadline)?;
        if bytes.is_empty() {
            return Ok(!line.is_empty());
        }
        let bytes = &bytes[..bytes.len().min(64 * 1024)];
        let newline = bytes.iter().position(|byte| *byte == b'\n');
        let count = newline.map_or(bytes.len(), |position| position + 1);
        line.extend_from_slice(&bytes[..count]);
        reader.consume(count);
        if newline.is_some() {
            return Ok(true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn deadline_is_checked_inside_an_unterminated_line() {
        struct Slow;
        impl Read for Slow {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                unreachable!()
            }
        }
        impl BufRead for Slow {
            fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
                std::thread::sleep(Duration::from_millis(25));
                Ok(b"PRIVATE never ending input")
            }
            fn consume(&mut self, _: usize) {}
        }
        let mut line = vec![];
        let error = read_line_until(
            &mut Slow,
            &mut line,
            Some(Instant::now() + Duration::from_millis(10)),
        )
        .unwrap_err();
        assert!(line.is_empty());
        assert!(!format!("{error:#}").contains("PRIVATE"));
    }

    #[test]
    fn deadline_between_records_never_returns_a_partial_result() {
        let record = serde_json::to_string(&crate::tests::sample_record()).unwrap();
        let raw = format!("{record}\n{record}\n");
        let mut visited = 0;
        let result = read_records_from_until(
            raw.as_bytes(),
            Path::new("fixture"),
            Some(Instant::now() + Duration::from_millis(10)),
            |_| {
                visited += 1;
                std::thread::sleep(Duration::from_millis(25));
                Ok(())
            },
        );
        assert!(result.is_err());
        assert!(visited <= 1);
    }
}
