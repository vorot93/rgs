extern crate failure;
extern crate futures_await as futures;
extern crate handlebars;
extern crate std;
extern crate tokio_timer;

use self::failure::Fail;

#[derive(Debug, Fail)]
pub enum Error {
    #[fail(display = "null error: {}", reason)]
    NullError { reason: String },
    #[fail(display = "data parse error: {}", reason)]
    DataParseError { reason: String },
    #[fail(display = "network error: {}", reason)]
    NetworkError { reason: String },
    #[fail(display = "invalid packet: {}", reason)]
    InvalidPacketError { reason: String },
    #[fail(display = "IO error: {}", reason)]
    IOError { reason: String },
    #[fail(display = "pipe error: {}", reason)]
    PipeError { reason: String },
    #[fail(display = "operation timed out: {}", reason)]
    TimeoutError { reason: String },
}

impl From<std::option::NoneError> for Error {
    fn from(v: std::option::NoneError) -> Self {
        Error::NullError { reason: "".into() }
    }
}

impl<T: 'static> From<futures::sync::mpsc::SendError<T>> for Error {
    fn from(v: futures::sync::mpsc::SendError<T>) -> Self {
        Error::PipeError {
            reason: std::error::Error::description(&v).into(),
        }
    }
}

impl<T: 'static> From<tokio_timer::TimeoutError<T>> for Error {
    fn from(v: tokio_timer::TimeoutError<T>) -> Self {
        Error::TimeoutError {
            reason: std::error::Error::description(&v).into(),
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;
