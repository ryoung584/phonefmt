//! Streaming extraction of phone-number candidates from arbitrary
//! input. The point of this module is the memory bound: no matter how
//! large the input is, we hold at most one fixed-size read buffer plus
//! one in-progress candidate string at a time, so this is safe to
//! point at a multi-gigabyte file or a long-lived socket.

use crate::parser::{self, ParseError, PhoneNumber};
use std::io::{self, Read};

const BUF_SIZE: usize = 8 * 1024;

/// Characters we consider part of a phone-number-looking run. Once a
/// character outside this set (or the end of input) shows up, whatever
/// has been accumulated so far is handed to the parser.
fn is_candidate_char(c: char) -> bool {
    c.is_ascii_digit() || matches!(c, '+' | '-' | '.' | '(' | ')' | ' ')
}

/// Scan `reader` for phone-number candidates and invoke `on_match` for
/// each one, successful or not, in the order encountered. `on_match`
/// receives the raw candidate text alongside the parse result.
pub fn scan<R: Read, F: FnMut(&str, Result<PhoneNumber, ParseError>)>(
    mut reader: R,
    mut on_match: F,
) -> io::Result<()> {
    let mut buf = [0u8; BUF_SIZE];
    let mut candidate = String::new();
    // Bytes at the end of a chunk that didn't form a complete UTF-8
    // character, carried over so we don't lose or mangle them.
    let mut leftover: Vec<u8> = Vec::new();

    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }

        let mut chunk = Vec::with_capacity(leftover.len() + n);
        chunk.append(&mut leftover);
        chunk.extend_from_slice(&buf[..n]);

        let text = match std::str::from_utf8(&chunk) {
            Ok(s) => s,
            Err(e) => {
                let valid_up_to = e.valid_up_to();
                leftover.extend_from_slice(&chunk[valid_up_to..]);
                std::str::from_utf8(&chunk[..valid_up_to]).expect("checked above")
            }
        };

        for c in text.chars() {
            if is_candidate_char(c) {
                candidate.push(c);
            } else if !candidate.is_empty() {
                flush(&mut candidate, &mut on_match);
            }
        }
    }

    if !candidate.is_empty() {
        flush(&mut candidate, &mut on_match);
    }

    Ok(())
}

fn flush<F: FnMut(&str, Result<PhoneNumber, ParseError>)>(
    candidate: &mut String,
    on_match: &mut F,
) {
    let digit_count = candidate.chars().filter(|c| c.is_ascii_digit()).count();
    if digit_count >= 7 {
        let result = parser::parse(candidate);
        on_match(candidate, result);
    }
    candidate.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_one_number_in_prose() {
        let input = "reach the front desk at (212) 555-0143 during business hours";
        let mut found = Vec::new();
        scan(input.as_bytes(), |raw, result| found.push((raw.to_string(), result))).unwrap();
        assert_eq!(found.len(), 1);
        assert!(found[0].1.is_ok());
    }

    #[test]
    fn candidate_can_span_read_buffer_boundary() {
        // A reader that only ever hands back one byte per call, to
        // force the scanner to reassemble a candidate across many
        // small reads.
        struct OneByteAtATime<'a>(&'a [u8]);
        impl<'a> Read for OneByteAtATime<'a> {
            fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
                if self.0.is_empty() || buf.is_empty() {
                    return Ok(0);
                }
                buf[0] = self.0[0];
                self.0 = &self.0[1..];
                Ok(1)
            }
        }

        let input = b"call 212-555-0143 now";
        let mut found = Vec::new();
        scan(OneByteAtATime(input), |raw, result| {
            found.push((raw.to_string(), result))
        })
        .unwrap();
        assert_eq!(found.len(), 1);
        assert!(found[0].1.is_ok());
    }

    #[test]
    fn ignores_short_runs_of_digits() {
        let input = "room 42, ext 5";
        let mut found = Vec::new();
        scan(input.as_bytes(), |raw, result| found.push((raw.to_string(), result))).unwrap();
        assert!(found.is_empty());
    }
}
