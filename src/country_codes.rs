//! Table of country calling codes assigned under ITU-T recommendation
//! E.164, used to split an international number into country calling
//! code and national number by longest matching prefix. Replaces the
//! old guess of "the first three digits are the country code."

/// Every assigned calling code, one to three digits, as ASCII digit
/// strings. Codes are prefix-free by construction: no assigned code is
/// itself a prefix of a different assigned code, so at most one length
/// tried by `split` will ever match a given number.
const CODES: &[&str] = &[
    // zone 1: NANP (US, Canada, and 19 Caribbean nations). The caller
    // handles this case before consulting the table in the common case
    // of an 11-digit number, but it's listed here too so a NANP number
    // of unusual length still splits instead of being rejected.
    "1",
    // zone 2: Africa, plus a handful of island territories
    "20", "211", "212", "213", "216", "218", "220", "221", "222", "223",
    "224", "225", "226", "227", "228", "229", "230", "231", "232", "233",
    "234", "235", "236", "237", "238", "239", "240", "241", "242", "243",
    "244", "245", "246", "247", "248", "249", "250", "251", "252", "253",
    "254", "255", "256", "257", "258", "260", "261", "262", "263", "264",
    "265", "266", "267", "268", "269", "27", "290", "291", "297", "298",
    "299",
    // zone 3/4: Europe
    "30", "31", "32", "33", "34", "350", "351", "352", "353", "354",
    "355", "356", "357", "358", "359", "36", "370", "371", "372", "373",
    "374", "375", "376", "377", "378", "379", "380", "381", "382", "383",
    "385", "386", "387", "389", "39", "40", "41", "420", "421", "423",
    "43", "44", "45", "46", "47", "48", "49",
    // zone 5: Mexico, Central and South America, the Caribbean
    "500", "501", "502", "503", "504", "505", "506", "507", "508", "509",
    "51", "52", "53", "54", "55", "56", "57", "58", "590", "591", "592",
    "593", "594", "595", "596", "597", "598", "599",
    // zone 6: Southeast Asia and Oceania (Guam and American Samoa are
    // NANP territories under code 1, not listed separately here)
    "60", "61", "62", "63", "64", "65", "66", "670", "672", "673", "674",
    "675", "676", "677", "678", "679", "680", "681", "682", "683", "685",
    "686", "687", "688", "689", "690", "691", "692",
    // zone 7: Russia and Kazakhstan
    "7",
    // zone 8: East Asia, plus non-geographic global services
    "800", "808", "81", "82", "84", "850", "852", "853", "855", "856",
    "86", "870", "878", "880", "881", "882", "883", "886", "888",
    // zone 9: South, West, and Central Asia, the Middle East
    "90", "91", "92", "93", "94", "95", "960", "961", "962", "963",
    "964", "965", "966", "967", "968", "970", "971", "972", "973", "974",
    "975", "976", "977", "979", "98", "992", "993", "994", "995", "996",
    "998",
];

/// Split `digits` (already stripped of any leading `+` and all
/// punctuation) into a country calling code and the remaining national
/// number, trying the three-digit prefix first, then two, then one.
/// Returns `None` if no assigned code matches.
pub fn split(digits: &str) -> Option<(u16, &str)> {
    for len in [3usize, 2, 1] {
        if digits.len() <= len {
            continue;
        }
        let prefix = &digits[..len];
        if CODES.contains(&prefix) {
            let code = prefix.parse().expect("prefix is ascii digits");
            return Some((code, &digits[len..]));
        }
    }
    None
}

/// Digit-grouping used when printing a national number, keyed by
/// (country calling code, national number length). Kept to countries
/// where a number of that length has one unambiguous, fixed grouping
/// in ordinary use, so this never has to guess. Anything not listed
/// here falls back to the plain, ungrouped form.
pub fn group_sizes(country_code: u16, national_len: usize) -> Option<&'static [usize]> {
    match (country_code, national_len) {
        (33, 9) => Some(&[1, 2, 2, 2, 2]),  // France: 6 12 34 56 78
        (34, 9) => Some(&[3, 3, 3]),        // Spain: 612 345 678
        (7, 10) => Some(&[3, 3, 2, 2]),     // Russia/Kazakhstan: 916 123 45 67
        (86, 11) => Some(&[3, 4, 4]),       // China mobile: 138 0013 8000
        (91, 10) => Some(&[5, 5]),          // India: 98765 43210
        (52, 10) => Some(&[3, 3, 4]),       // Mexico: 551 234 5678
        (55, 11) => Some(&[2, 5, 4]),       // Brazil mobile: 11 98765 4321
        (55, 10) => Some(&[2, 4, 4]),       // Brazil landline: 11 2345 6789
        (27, 9) => Some(&[2, 3, 4]),        // South Africa: 82 123 4567
        (65, 8) => Some(&[4, 4]),           // Singapore: 6123 4567
        (852, 8) => Some(&[4, 4]),          // Hong Kong: 6123 4567
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_two_digit_code_over_shorter_or_longer_guess() {
        // "44" (UK) is real; neither "4" nor "447" is an assigned code,
        // so the two-digit prefix must be the one that wins.
        assert_eq!(split("442071234567"), Some((44, "2071234567")));
    }

    #[test]
    fn matches_three_digit_code() {
        assert_eq!(split("212612345678"), Some((212, "612345678")));
    }

    #[test]
    fn matches_single_digit_code() {
        assert_eq!(split("79161234567"), Some((7, "9161234567")));
    }

    #[test]
    fn rejects_unassigned_prefix() {
        assert_eq!(split("9991234567"), None);
    }

    #[test]
    fn rejects_prefix_with_nothing_left_over() {
        // The whole string being a valid code with no digits left for a
        // national number isn't a real phone number.
        assert_eq!(split("44"), None);
    }

    #[test]
    fn group_sizes_sum_to_the_length_they_are_keyed_on() {
        let lens_to_check: &[(u16, usize)] = &[
            (33, 9),
            (34, 9),
            (7, 10),
            (86, 11),
            (91, 10),
            (52, 10),
            (55, 11),
            (55, 10),
            (27, 9),
            (65, 8),
            (852, 8),
        ];
        for &(cc, len) in lens_to_check {
            let groups = group_sizes(cc, len).unwrap();
            assert_eq!(groups.iter().sum::<usize>(), len, "cc {cc} len {len}");
        }
    }

    #[test]
    fn unlisted_country_has_no_grouping() {
        assert_eq!(group_sizes(44, 10), None);
    }
}
