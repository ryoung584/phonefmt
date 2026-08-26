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
Everything else currently only gets an E.164 length check plus a
best-effort split into country code and national number — see
[Limitations](#limitations).

## Usage

As a library:

```rust
use phonefmt::parse;

let number = parse("(212) 555-0143").unwrap();
assert_eq!(number.to_string(), "+1 (212) 555-0143");

assert!(parse("(112) 555-0143").is_err()); // area code can't start with 1
```

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
  length check, and the country-code/national-number split for them is
  a guess (first three digits), not backed by a real prefix table.
- The scanner's candidate boundary is "run of digits, `+ - . ( ) space`
  characters." Two numbers separated only by a single space will be
  read as one candidate and likely fail to parse. See the roadmap in
  the commit history for planned fixes.

## License

MIT, see [LICENSE](LICENSE).
