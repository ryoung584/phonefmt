//! Validating parser for phone numbers.
//!
//! NANP (+1) numbers get checked against the actual area-code and
//! exchange-code rules. Everything else gets its country calling code
//! split off with the real ITU-T E.164 prefix table (see
//! `country_codes`) and then only an overall length check, since
//! validating the national number itself would require each country's
//! own numbering plan.

use crate::country_codes;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhoneNumber {
    pub country_code: u16,
    pub national_number: String,
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
/// punctuation a human would type: spaces, dashes, dots, parens.
pub fn parse(raw: &str) -> Result<PhoneNumber, ParseError> {
    let has_plus = raw.trim_start().starts_with('+');
    let digits: String = raw.chars().filter(|c| c.is_ascii_digit()).collect();

    if digits.is_empty() {
        return Err(ParseError::NoDigits);
    }
    if digits.len() > 15 {
        return Err(ParseError::TooLong);
    }

    if has_plus {
        return parse_e164(&digits);
    }

    // No leading '+': assume NANP, since that's the only plan we
    // validate in full right now.
    match digits.len() {
        10 => parse_nanp(&digits, 1),
        11 if digits.starts_with('1') => parse_nanp(&digits[1..], 1),
        n if n < 8 => Err(ParseError::TooShort),
        _ => parse_e164(&digits),
    }
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
            )
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
            Ok(())
        } else {
            write!(f, "+{} {}", self.country_code, self.national_number)
        }
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
}
