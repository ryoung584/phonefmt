# phonefmt

A validating parser and pretty printer for phone numbers, plus a
scanner that pulls candidates out of a stream of text without ever
loading the whole input into memory.

## Why

Most "phone number regex" code either accepts almost anything with the
right digit count, or drags in a multi-megabyte library of every
country's numbering plan. This is the middle ground I actually want
most of the time: real validation for North American numbers (area
code and exchange code rules, not just "10 digits"), a sane length
check for everything else, and a scanner I can point at a log file or
a pipe without worrying about memory.

## Status

North American Numbering Plan (+1) numbers are fully validated.
Everything else gets its country calling code split off using the real
ITU-T E.164 assignment table (longest-prefix match, not a guess), plus
an overall length check — see [Limitations](#limitations).

## Usage

As a library:

```rust
use phonefmt::parse;

let number = parse("(212) 555-0143").unwrap();
assert_eq!(number.to_string(), "+1 (212) 555-0143");

assert!(parse("(112) 555-0143").is_err()); // area code can't start with 1

let with_ext = parse("212-555-0143 ext. 1234").unwrap();
assert_eq!(with_ext.extension, Some("1234".to_string()));
assert_eq!(with_ext.to_string(), "+1 (212) 555-0143 ext. 1234");
```

Extensions are recognized as `x1234`, `ext 1234`, `ext. 1234`, or
`extension 1234`, anchored to the end of the input.

Scanning a stream for every candidate number in it:

```rust
use phonefmt::scan;
use std::io::Cursor;

let input = Cursor::new("call the office at 212-555-0143 or 800-555-0199");

scan(input, |raw, result| match result {
    Ok(number) => println!("{raw} -> {number}"),
    Err(err) => eprintln!("{raw} -> invalid: {err}"),
}).unwrap();
```

`scan` takes anything that implements `std::io::Read`. It reads in
fixed-size chunks and never buffers more than one chunk plus one
in-progress candidate, so a multi-gigabyte file or a long-lived socket
works the same way a short string does.

## Command line

```sh
cargo run -- numbers.txt
cat call_log.txt | cargo run
```

Prints one line per candidate found, formatted number or parse error,
and exits non-zero if any candidate failed to parse.

## Limitations

- Only NANP numbers get full validation. Other countries only get a
  length check; the national number itself isn't checked against that
  country's own numbering plan. A handful of countries with a fixed,
  unambiguous digit grouping (France, Spain, Russia/Kazakhstan, China
  mobile, India, Mexico, Brazil, South Africa, Singapore, Hong Kong)
  print with that grouping; everything else still falls back to a
  plain `+cc national-number`.
- The scanner's candidate boundary is "run of digits, `+ - . ( ) space`
  characters," so two numbers separated only by a single space still
  end up in the same candidate run. When that whole run fails to parse,
  the scanner looks for the longest leading segment that does, reports
  it, and keeps going with what's left — but a pathological run that
  happens to parse as one (very) long invalid number won't get split.
- `parse` recognizes extensions, but the scanner doesn't yet: its
  candidate runs stop at the first letter, so `x1234` / `ext. 1234`
  after a scanned number is dropped rather than attached.

## License

MIT, see [LICENSE](LICENSE).
