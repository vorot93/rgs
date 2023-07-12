use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Null error")]
    NullError,
    #[error("Data parse error")]
    DataParseError,
    #[error("Network error")]
    NetworkError,
    #[error("Invalid packet")]
    InvalidPacketError,
    #[error("Pipe error")]
    PipeError,
    #[error("Operation timed out")]
    TimeoutError,
}
