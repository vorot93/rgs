extern crate std;

extern crate handlebars;

#[derive(Clone, Debug)]
pub enum Error {
    NullError(String),
    ChannelError(String),
    IOError(String),
    NetworkError(String),
    TimeoutError(String),
    InvalidPacketError(String),
    TemplateError(String),
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Error {
        match e.kind() {
            std::io::ErrorKind::TimedOut => Error::TimeoutError(format!("Timed out: {}", &e)),
            _ => Error::IOError("IO Failure: {}".into()),
        }
    }
}

impl From<handlebars::TemplateRenderError> for Error {
    fn from(_: handlebars::TemplateRenderError) -> Error {
        Error::TemplateError("Failed to parse handlebars template".into())
    }
}

impl From<std::sync::mpsc::RecvError> for Error {
    fn from(_: std::sync::mpsc::RecvError) -> Error {
        Error::ChannelError("Failed to receive message from channel".into())
    }
}

impl<T> From<std::sync::mpsc::SendError<T>> for Error {
    fn from(e: std::sync::mpsc::SendError<T>) -> Error {
        Error::ChannelError(format!("Failed to send message on channel: {}", e))
    }
}
