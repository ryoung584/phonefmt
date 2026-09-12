//! Validating parser for phone numbers.
//!
//! NANP (+1) numbers get checked against the actual area-code and
//! exchange-code rules. Everything else gets its country calling code
//! split off with the real ITU-T E.164 prefix table (see
//! `country_codes`) and then only an overall length check, since
//! validating the national number itself would require each country's
//! own numbering plan. A trailing extension (`x1234`, `ext. 1234`,
//! `extension 1234`) is split off before any of that and carried
//! alongside the parsed number rather than being folded into it.

use crate::country_codes;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhoneNumber {
    pub country_code: u16,
    pub national_number: String,
    pub extension: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    NoDigits,
    TooShort,
    TooLong,
    InvalidNanpAreaCode,
    InvalidNanpExchangeCode,
    UnknownCountryCode,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let msg = match self {
            ParseError::NoDigits => "no digits found",
            ParseError::TooShort => "fewer digits than any known plan allows",
            ParseError::TooLong => "more than the 15 digits E.164 allows",
            ParseError::InvalidNanpAreaCode => "NANP area code can't start with 0 or 1",
            ParseError::InvalidNanpExchangeCode => "NANP exchange code can't start with 0 or 1",
            ParseError::UnknownCountryCode => "digits don't start with any assigned country calling code",
        };
        f.write_str(msg)
    }
}

impl std::error::Error for ParseError {}

/// Parse a single phone number out of `raw`, which may still carry the
/// punctuation a human would type: spaces, dashes, dots, parens, and a
/// trailing extension.
pub fn parse(raw: &str) -> Result<PhoneNumber, ParseError> {
    let (main, extension) = split_extension(raw);

    let has_plus = main.trim_start().starts_with('+');
    let digits: String = main.chars().filter(|c| c.is_ascii_digit()).collect();

    if digits.is_empty() {
        return Err(ParseError::NoDigits);
    }
    if digits.len() > 15 {
        return Err(ParseError::TooLong);
    }

    let mut number = if has_plus {
        parse_e164(&digits)?
    } else {
        // No leading '+': assume NANP, since that's the only plan we
        // validate in full right now.
        match digits.len() {
            10 => parse_nanp(&digits, 1)?,
            11 if digits.starts_with('1') => parse_nanp(&digits[1..], 1)?,
            n if n < 8 => return Err(ParseError::TooShort),
            _ => parse_e164(&digits)?,
        }
    };
    number.extension = extension;
    Ok(number)
}

/// Split a trailing extension off of `raw`. Recognizes `x1234`,
/// `ext 1234`, `ext. 1234`, and `extension 1234` (case-insensitively),
/// anchored to the end of the string so a marker doesn't need a digit
/// immediately after it just to get skipped as a false positive.
/// Returns the text to parse as the main number, plus the extension
/// digits if a marker was actually found.
fn split_extension(raw: &str) -> (&str, Option<String>) {
    let lower = raw.to_ascii_lowercase();
    const MARKERS: [&str; 3] = ["extension", "ext", "x"];

    for marker in MARKERS {
        let mut search_start = 0;
        while let Some(rel_pos) = lower[search_start..].find(marker) {
            let pos = search_start + rel_pos;

            // Require the marker not be part of a longer word (e.g. the
            // "x" in "Box", or the "ext" in "Text") by checking that
            // whatever precedes it, if anything, isn't a letter.
            let before_ok = raw[..pos]
                .chars()
                .next_back()
                .map(|c| !c.is_ascii_alphabetic())
                .unwrap_or(true);

            let after = raw[pos + marker.len()..].trim_start_matches('.').trim_start();
            let ext_digits: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
            let trailing = &after[ext_digits.len()..];

            if before_ok && !ext_digits.is_empty() && trailing.trim().is_empty() {
                return (&raw[..pos], Some(ext_digits));
            }

            search_start = pos + marker.len();
        }
    }

    (raw, None)
}

fn parse_e164(digits: &str) -> Result<PhoneNumber, ParseError> {
    if digits.len() < 8 {
        return Err(ParseError::TooShort);
    }
    if digits.starts_with('1') && digits.len() == 11 {
        return parse_nanp(&digits[1..], 1);
    }
    match country_codes::split(digits) {
        Some((cc, national)) => Ok(PhoneNumber {
            country_code: cc,
            national_number: national.to_string(),
            extension: None,
        }),
        None => Err(ParseError::UnknownCountryCode),
    }
}

fn parse_nanp(national: &str, country_code: u16) -> Result<PhoneNumber, ParseError> {
    if national.len() != 10 {
        return Err(ParseError::TooShort);
    }
    let bytes = national.as_bytes();
    if bytes[0] == b'0' || bytes[0] == b'1' {
        return Err(ParseError::InvalidNanpAreaCode);
    }
    if bytes[3] == b'0' || bytes[3] == b'1' {
        return Err(ParseError::InvalidNanpExchangeCode);
    }
    Ok(PhoneNumber {
        country_code,
        national_number: national.to_string(),
        extension: None,
    })
}

impl fmt::Display for PhoneNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.country_code == 1 && self.national_number.len() == 10 {
            let n = self.national_number.as_bytes();
            write!(
                f,
                "+1 ({}{}{}) {}{}{}-{}{}{}{}",
                n[0] as char,
                n[1] as char,
                n[2] as char,
                n[3] as char,
                n[4] as char,
                n[5] as char,
                n[6] as char,
                n[7] as char,
                n[8] as char,
                n[9] as char,
            )?;
        } else if let Some(groups) =
            country_codes::group_sizes(self.country_code, self.national_number.len())
        {
            write!(f, "+{} ", self.country_code)?;
            let mut rest = self.national_number.as_str();
            for (i, &size) in groups.iter().enumerate() {
                if i > 0 {
                    f.write_str(" ")?;
                }
                let (head, tail) = rest.split_at(size);
                f.write_str(head)?;
                rest = tail;
            }
        } else {
            write!(f, "+{} {}", self.country_code, self.national_number)?;
        }

        if let Some(ext) = &self.extension {
            write!(f, " ext. {ext}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_nanp() {
        let n = parse("2125551234").unwrap();
        assert_eq!(n.country_code, 1);
        assert_eq!(n.national_number, "2125551234");
        assert_eq!(n.to_string(), "+1 (212) 555-1234");
    }

    #[test]
    fn parses_formatted_nanp() {
        let n = parse("+1 (212) 555-1234").unwrap();
        assert_eq!(n.to_string(), "+1 (212) 555-1234");
    }

    #[test]
    fn rejects_bad_area_code() {
        assert_eq!(parse("0125551234"), Err(ParseError::InvalidNanpAreaCode));
    }

    #[test]
    fn rejects_bad_exchange_code() {
        assert_eq!(parse("2121051234"), Err(ParseError::InvalidNanpExchangeCode));
    }

    #[test]
    fn rejects_too_few_digits() {
        assert_eq!(parse("555-1234"), Err(ParseError::TooShort));
    }

    #[test]
    fn rejects_no_digits() {
        assert_eq!(parse("call me"), Err(ParseError::NoDigits));
    }

    #[test]
    fn splits_two_digit_country_code() {
        let n = parse("+44 20 7946 0958").unwrap();
        assert_eq!(n.country_code, 44);
        assert_eq!(n.national_number, "2079460958");
        assert_eq!(n.to_string(), "+44 2079460958");
    }

    #[test]
    fn splits_three_digit_country_code() {
        let n = parse("+212 6 12 34 56 78").unwrap();
        assert_eq!(n.country_code, 212);
        assert_eq!(n.national_number, "612345678");
    }

    #[test]
    fn rejects_unassigned_country_code() {
        assert_eq!(
            parse("+999 123 4567"),
            Err(ParseError::UnknownCountryCode)
        );
    }

    #[test]
    fn formats_french_number_with_national_grouping() {
        let n = parse("+33 6 12 34 56 78").unwrap();
        assert_eq!(n.to_string(), "+33 6 12 34 56 78");
    }

    #[test]
    fn formats_indian_number_with_national_grouping() {
        let n = parse("+91 9876543210").unwrap();
        assert_eq!(n.to_string(), "+91 98765 43210");
    }

    #[test]
    fn formats_brazilian_mobile_and_landline_differently() {
        let mobile = parse("+55 11987654321").unwrap();
        assert_eq!(mobile.to_string(), "+55 11 98765 4321");

        let landline = parse("+55 1123456789").unwrap();
        assert_eq!(landline.to_string(), "+55 11 2345 6789");
    }

    #[test]
    fn unlisted_country_still_falls_back_to_plain_form() {
        // UK (44) has no fixed-length grouping entry, so it keeps the
        // generic "+cc national-number" form.
        let n = parse("+44 20 7946 0958").unwrap();
        assert_eq!(n.to_string(), "+44 2079460958");
    }

    #[test]
    fn parses_x_style_extension() {
        let n = parse("212-555-0143x1234").unwrap();
        assert_eq!(n.national_number, "2125550143");
        assert_eq!(n.extension, Some("1234".to_string()));
        assert_eq!(n.to_string(), "+1 (212) 555-0143 ext. 1234");
    }

    #[test]
    fn parses_x_style_extension_with_leading_space() {
        let n = parse("212-555-0143 x1234").unwrap();
        assert_eq!(n.extension, Some("1234".to_string()));
        assert_eq!(n.national_number, "2125550143");
    }

    #[test]
    fn parses_ext_dot_style_extension() {
        let n = parse("212-555-0143 ext. 1234").unwrap();
        assert_eq!(n.extension, Some("1234".to_string()));
        assert_eq!(n.national_number, "2125550143");
    }

    #[test]
    fn parses_ext_style_extension_without_dot() {
        let n = parse("212-555-0143 ext 1234").unwrap();
        assert_eq!(n.extension, Some("1234".to_string()));
    }

    #[test]
    fn parses_extension_word_style_extension() {
        let n = parse("212-555-0143 extension 1234").unwrap();
        assert_eq!(n.extension, Some("1234".to_string()));
    }

    #[test]
    fn extension_marker_inside_a_word_is_not_mistaken_for_one() {
        // "Text" contains "ext", but it isn't a real extension marker
        // since a letter comes right before it.
        let n = parse("Text 2125550143").unwrap();
        assert_eq!(n.extension, None);
        assert_eq!(n.national_number, "2125550143");
    }

    #[test]
    fn number_without_extension_has_none() {
        let n = parse("2125550143").unwrap();
        assert_eq!(n.extension, None);
    }
}
