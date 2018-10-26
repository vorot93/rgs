use failure::Fail;

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
