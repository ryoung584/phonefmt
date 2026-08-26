use std::env;
use std::fs::File;
use std::io::{self, stdin, BufReader, Read};
use std::process::ExitCode;

use phonefmt::scan;

fn main() -> ExitCode {
    let path = env::args().nth(1);

    let outcome = match path {
        Some(path) => File::open(&path).and_then(|f| run(BufReader::new(f))),
        None => run(BufReader::new(stdin())),
    };

    match outcome {
        Ok(0) => ExitCode::SUCCESS,
        Ok(_) => ExitCode::from(1),
        Err(e) => {
            eprintln!("phonefmt: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Streams `reader` through the scanner, printing one line per
/// candidate found. Returns the number of candidates that failed to
/// parse, which main() uses as the process exit status.
fn run<R: Read>(reader: R) -> io::Result<u32> {
    let mut bad = 0;
    scan(reader, |raw, result| match result {
        Ok(number) => println!("{raw} -> {number}"),
        Err(err) => {
            bad += 1;
            eprintln!("{raw} -> invalid: {err}");
        }
    })?;
    Ok(bad)
}
