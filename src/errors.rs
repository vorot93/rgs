extern crate failure;
extern crate handlebars;
extern crate std;

use self::failure::Fail;

#[derive(Debug, Fail)]
pub enum Error {
    #[fail(display = "null error: {}", what)]
    NullError { what: String },
    #[fail(display = "data parse error: {}", what)]
    DataParseError { what: String },
    #[fail(display = "network error: {}", what)]
    NetworkError { what: String },
    #[fail(display = "timeout error: {}", what)]
    TimeoutError { what: String },
    #[fail(display = "invalid packet: {}", what)]
    InvalidPacketError { what: String },
    #[fail(display = "IO error: {}", what)]
    IOError { what: String },
}

pub type Result<T> = std::result::Result<T, Error>;
