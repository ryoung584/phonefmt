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

/// Below this many digits a run isn't worth handing to the parser at
/// all (matches the shortest national numbers we validate).
const MIN_CANDIDATE_DIGITS: usize = 7;

fn digit_count(s: &str) -> usize {
    s.chars().filter(|c| c.is_ascii_digit()).count()
}

/// Parse whatever accumulated in `candidate`, which may actually be two
/// or more numbers that got joined by a single space acting as a
/// separator rather than internal grouping (e.g. two office extensions
/// listed as "212-555-0143 800-555-0199"). If the whole run doesn't
/// parse, look for the longest leading segment ending at a space that
/// does, report it, and keep working through what's left, so a
/// single-space gap between numbers no longer drags them both down.
fn flush<F: FnMut(&str, Result<PhoneNumber, ParseError>)>(
    candidate: &mut String,
    on_match: &mut F,
) {
    let mut remaining = candidate.as_str();

    loop {
        let text = remaining.trim_matches(' ');
        if text.is_empty() || digit_count(text) < MIN_CANDIDATE_DIGITS {
            break;
        }

        if let Ok(number) = parser::parse(text) {
            on_match(text, Ok(number));
            break;
        }

        let space_positions = text.char_indices().filter(|&(_, c)| c == ' ').map(|(i, _)| i);
        let mut split = None;
        for pos in space_positions.collect::<Vec<_>>().into_iter().rev() {
            let head = &text[..pos];
            if digit_count(head) < MIN_CANDIDATE_DIGITS {
                continue;
            }
            if let Ok(number) = parser::parse(head) {
                on_match(head, Ok(number));
                split = Some(pos);
                break;
            }
        }

        match split {
            Some(pos) => remaining = &text[pos + 1..],
            None => {
                on_match(text, parser::parse(text));
                break;
            }
        }
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

    #[test]
    fn splits_two_numbers_joined_by_a_single_space() {
        let input = "212-555-0143 800-555-0199";
        let mut found = Vec::new();
        scan(input.as_bytes(), |raw, result| found.push((raw.to_string(), result))).unwrap();
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].0, "212-555-0143");
        assert!(found[0].1.is_ok());
        assert_eq!(found[1].0, "800-555-0199");
        assert!(found[1].1.is_ok());
    }

    #[test]
    fn splits_three_numbers_joined_by_single_spaces() {
        let input = "212-555-0143 800-555-0199 415-555-0100";
        let mut found = Vec::new();
        scan(input.as_bytes(), |raw, result| found.push((raw.to_string(), result))).unwrap();
        let raws: Vec<&str> = found.iter().map(|(raw, _)| raw.as_str()).collect();
        assert_eq!(raws, vec!["212-555-0143", "800-555-0199", "415-555-0100"]);
        assert!(found.iter().all(|(_, result)| result.is_ok()));
    }

    #[test]
    fn does_not_split_a_single_number_with_internal_grouping_spaces() {
        // "+44 20 7946 0958" is one number whose groups happen to be
        // space-separated; it must not be chopped into pieces.
        let input = "call +44 20 7946 0958 now";
        let mut found = Vec::new();
        scan(input.as_bytes(), |raw, result| found.push((raw.to_string(), result))).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].0, "+44 20 7946 0958");
        assert!(found[0].1.is_ok());
    }

    #[test]
    fn joined_valid_and_invalid_number_reports_both() {
        let input = "212-555-0143 000-000-0000";
        let mut found = Vec::new();
        scan(input.as_bytes(), |raw, result| found.push((raw.to_string(), result))).unwrap();
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].0, "212-555-0143");
        assert!(found[0].1.is_ok());
        assert_eq!(found[1].0, "000-000-0000");
        assert!(found[1].1.is_err());
    }
}
