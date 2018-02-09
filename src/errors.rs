extern crate failure;
extern crate futures_await as futures;
extern crate handlebars;
extern crate std;

use self::failure::Fail;

#[derive(Debug, Fail)]
pub enum Error {
    #[fail(display = "null error: {}", reason)]
    NullError { reason: String },
    #[fail(display = "data parse error: {}", reason)]
    DataParseError { reason: String },
    #[fail(display = "network error: {}", reason)]
    NetworkError { reason: String },
    #[fail(display = "timeout error: {}", reason)]
    TimeoutError { reason: String },
    #[fail(display = "invalid packet: {}", reason)]
    InvalidPacketError { reason: String },
    #[fail(display = "IO error: {}", reason)]
    IOError { reason: String },
    #[fail(display = "pipe error: {}", reason)]
    PipeError { reason: String },
}

impl<T> From<futures::sync::mpsc::SendError<T>> for Error {
    fn from(v: futures::sync::mpsc::SendError<T>) -> Self {
        Error::PipeError {
            reason: std::error::Error::description(&v).into(),
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;
