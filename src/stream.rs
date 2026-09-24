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
/// has been accumulated so far is handed to the parser. Letters aren't
/// in this set - a trailing extension marker is recognized separately
/// by `ExtState` so ordinary prose doesn't get pulled into candidates.
fn is_candidate_char(c: char) -> bool {
    c.is_ascii_digit() || matches!(c, '+' | '-' | '.' | '(' | ')' | ' ')
}

/// The longest extension marker `parser::split_extension` recognizes.
/// "ext" is a prefix of it, so matching has to stay open past "ext"
/// until either the word completes or the next character rules it out.
const EXTENSION_MARKER: &str = "extension";

/// Whether appending `c` to a marker attempt that has already matched
/// `matched_len` characters of `EXTENSION_MARKER` would still keep it a
/// valid (case-insensitive) prefix.
fn extends_extension_marker(matched_len: usize, c: char) -> bool {
    c.is_ascii_alphabetic()
        && EXTENSION_MARKER
            .as_bytes()
            .get(matched_len)
            .map_or(false, |&b| b == c.to_ascii_lowercase() as u8)
}

/// Tracks an in-progress attempt to recognize an extension marker
/// (`x`, `ext`, `ext.`, `extension`) right after a number, so its
/// digits can be folded into the same candidate instead of getting cut
/// off by the first non-digit, non-punctuation character.
enum ExtState {
    /// Not currently looking at a possible marker.
    Idle,
    /// Letters seen so far are still a valid prefix of "extension" (and
    /// not yet a complete marker on their own).
    Matching(String),
    /// A marker word has been confirmed; `raw` holds it plus whatever
    /// optional dots/spaces and digits have followed, `digits` holds
    /// just the digits. An extension is only real once `digits` is
    /// non-empty.
    AfterMarker { raw: String, digits: String },
}

/// Decide whether `c`, arriving right after an accumulated candidate,
/// could be the start of an extension marker.
fn try_start_marker(c: char) -> Option<ExtState> {
    if c.eq_ignore_ascii_case(&'x') {
        Some(ExtState::AfterMarker { raw: c.to_string(), digits: String::new() })
    } else if extends_extension_marker(0, c) {
        Some(ExtState::Matching(c.to_string()))
    } else {
        None
    }
}

/// Feed one character through the candidate/extension state machine.
/// May recurse once or twice to re-evaluate the same character after a
/// state transition (e.g. once a marker attempt is abandoned, `c` still
/// needs to be handled as ordinary input).
fn handle_char<F: FnMut(&str, Result<PhoneNumber, ParseError>)>(
    c: char,
    candidate: &mut String,
    ext: &mut ExtState,
    on_match: &mut F,
) {
    match ext {
        ExtState::Idle => {
            if is_candidate_char(c) {
                candidate.push(c);
            } else if !candidate.is_empty() {
                match try_start_marker(c) {
                    Some(state) => *ext = state,
                    None => flush(candidate, on_match),
                }
            }
        }
        ExtState::Matching(raw) => {
            if extends_extension_marker(raw.len(), c) {
                raw.push(c);
                if raw.eq_ignore_ascii_case(EXTENSION_MARKER) {
                    let raw = std::mem::take(raw);
                    *ext = ExtState::AfterMarker { raw, digits: String::new() };
                }
            } else if raw.eq_ignore_ascii_case("ext") {
                let raw = std::mem::take(raw);
                *ext = ExtState::AfterMarker { raw, digits: String::new() };
                handle_char(c, candidate, ext, on_match);
            } else {
                *ext = ExtState::Idle;
                flush(candidate, on_match);
                handle_char(c, candidate, ext, on_match);
            }
        }
        ExtState::AfterMarker { raw, digits } => {
            if c.is_ascii_digit() {
                raw.push(c);
                digits.push(c);
            } else if digits.is_empty() && (c == '.' || c == ' ') {
                raw.push(c);
            } else if !digits.is_empty() {
                let raw = std::mem::take(raw);
                candidate.push_str(&raw);
                *ext = ExtState::Idle;
                handle_char(c, candidate, ext, on_match);
            } else {
                *ext = ExtState::Idle;
                flush(candidate, on_match);
                handle_char(c, candidate, ext, on_match);
            }
        }
    }
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
    let mut ext = ExtState::Idle;
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

        // A chunk can contain more than one bad spot, and a bad byte
        // partway through must not poison everything after it: only a
        // sequence truncated by the true end of the chunk (error_len ==
        // None) is worth holding onto, since more bytes next read might
        // complete it. A sequence that's simply malformed (error_len ==
        // Some) will never become valid no matter what arrives later,
        // so it's skipped and decoding resumes right after it - keeping
        // `leftover` bounded instead of letting one bad byte turn every
        // later read into another zero-progress pass over the same
        // ever-growing buffer.
        let mut start = 0;
        loop {
            match std::str::from_utf8(&chunk[start..]) {
                Ok(s) => {
                    for c in s.chars() {
                        handle_char(c, &mut candidate, &mut ext, &mut on_match);
                    }
                    break;
                }
                Err(e) => {
                    let valid_up_to = e.valid_up_to();
                    let text = std::str::from_utf8(&chunk[start..start + valid_up_to])
                        .expect("checked above");
                    for c in text.chars() {
                        handle_char(c, &mut candidate, &mut ext, &mut on_match);
                    }

                    match e.error_len() {
                        Some(bad_len) => {
                            handle_char('\u{FFFD}', &mut candidate, &mut ext, &mut on_match);
                            start += valid_up_to + bad_len;
                        }
                        None => {
                            leftover.extend_from_slice(&chunk[start + valid_up_to..]);
                            break;
                        }
                    }
                }
            }
        }
    }

    // A marker that got all the way to collecting digits (but hasn't
    // hit a terminating character yet, since input just ran out) is a
    // real extension; fold it in before the final flush. Anything less
    // complete than that gets dropped, same as mid-stream abandonment.
    if let ExtState::AfterMarker { raw, digits } = &ext {
        if !digits.is_empty() {
            candidate.push_str(raw);
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
    fn captures_x_style_extension() {
        let input = "call 212-555-0143x1234 now";
        let mut found = Vec::new();
        scan(input.as_bytes(), |raw, result| found.push((raw.to_string(), result))).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].0, "212-555-0143x1234");
        let number = found[0].1.as_ref().unwrap();
        assert_eq!(number.extension, Some("1234".to_string()));
    }

    #[test]
    fn captures_ext_dot_style_extension_with_space_before_it() {
        let input = "front desk 212-555-0143 ext. 1234 during business hours";
        let mut found = Vec::new();
        scan(input.as_bytes(), |raw, result| found.push((raw.to_string(), result))).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].0, "212-555-0143 ext. 1234");
        let number = found[0].1.as_ref().unwrap();
        assert_eq!(number.extension, Some("1234".to_string()));
    }

    #[test]
    fn captures_extension_word_style_extension() {
        let input = "212-555-0143 extension 1234";
        let mut found = Vec::new();
        scan(input.as_bytes(), |raw, result| found.push((raw.to_string(), result))).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].0, "212-555-0143 extension 1234");
        let number = found[0].1.as_ref().unwrap();
        assert_eq!(number.extension, Some("1234".to_string()));
    }

    #[test]
    fn extension_survives_across_read_buffer_boundary() {
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

        let input = b"212-555-0143 ext 1234";
        let mut found = Vec::new();
        scan(OneByteAtATime(input), |raw, result| {
            found.push((raw.to_string(), result))
        })
        .unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].0, "212-555-0143 ext 1234");
        let number = found[0].1.as_ref().unwrap();
        assert_eq!(number.extension, Some("1234".to_string()));
    }

    #[test]
    fn word_starting_like_a_marker_does_not_swallow_a_second_number() {
        // "extra" starts the same way "extension" does; the marker
        // attempt should get abandoned rather than eating the first
        // number or the one that follows it.
        let input = "212-555-0143 extra 800-555-0199";
        let mut found = Vec::new();
        scan(input.as_bytes(), |raw, result| found.push((raw.to_string(), result))).unwrap();
        let raws: Vec<&str> = found.iter().map(|(raw, _)| raw.as_str()).collect();
        assert_eq!(raws, vec!["212-555-0143", "800-555-0199"]);
        assert!(found.iter().all(|(_, result)| result.is_ok()));
    }

    #[test]
    fn extension_on_first_number_does_not_block_split_from_second() {
        let input = "212-555-0143 x1234 800-555-0199";
        let mut found = Vec::new();
        scan(input.as_bytes(), |raw, result| found.push((raw.to_string(), result))).unwrap();
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].0, "212-555-0143 x1234");
        assert_eq!(found[0].1.as_ref().unwrap().extension, Some("1234".to_string()));
        assert_eq!(found[1].0, "800-555-0199");
        assert!(found[1].1.is_ok());
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

    #[test]
    fn invalid_utf8_byte_does_not_swallow_the_rest_of_the_stream() {
        let mut input = Vec::new();
        input.extend_from_slice(b"212-555-0143 ");
        input.push(0xFF); // never valid UTF-8, on its own or continued
        input.extend_from_slice(b" 800-555-0199");

        let mut found = Vec::new();
        scan(input.as_slice(), |raw, result| found.push((raw.to_string(), result))).unwrap();

        let raws: Vec<&str> = found.iter().map(|(raw, _)| raw.as_str()).collect();
        assert_eq!(raws, vec!["212-555-0143", "800-555-0199"]);
        assert!(found.iter().all(|(_, result)| result.is_ok()));
    }

    #[test]
    fn invalid_utf8_byte_split_across_many_small_reads_does_not_swallow_the_rest() {
        // With one byte per read, every read after the bad byte used to
        // rebuild `chunk` starting with that same bad byte, so
        // `valid_up_to()` came back 0 forever and the rest of the input
        // just piled up in `leftover` and was thrown away at EOF.
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

        let mut input = Vec::new();
        input.extend_from_slice(b"212-555-0143 ");
        input.push(0xFF);
        input.extend_from_slice(b" 800-555-0199");

        let mut found = Vec::new();
        scan(OneByteAtATime(&input), |raw, result| {
            found.push((raw.to_string(), result))
        })
        .unwrap();

        let raws: Vec<&str> = found.iter().map(|(raw, _)| raw.as_str()).collect();
        assert_eq!(raws, vec!["212-555-0143", "800-555-0199"]);
        assert!(found.iter().all(|(_, result)| result.is_ok()));
    }
}
