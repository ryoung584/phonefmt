pub mod parser;
pub mod stream;

pub use parser::{parse, ParseError, PhoneNumber};
pub use stream::scan;
