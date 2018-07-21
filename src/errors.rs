use failure;
use std;

#[derive(Debug, Fail)]
pub enum Error {
    #[fail(display = "Null error")]
    NullError,
    #[fail(display = "Data parse error")]
    DataParseError,
    #[fail(display = "Network error")]
    NetworkError,
    #[fail(display = "Invalid packet")]
    InvalidPacketError,
    #[fail(display = "Pipe error")]
    PipeError,
    #[fail(display = "Operation timed out")]
    TimeoutError,
}

pub type Result<T> = std::result::Result<T, failure::Error>;