extern crate std;

extern crate handlebars;

error_chain!{
    foreign_links {
        TemplateError(handlebars::TemplateRenderError);
        ChannelRecvError(std::sync::mpsc::RecvError);
        IOError(std::io::Error);
        AddrParseError(std::net::AddrParseError);
    }
    errors {
        NullError(t: String) {
            description(""),
            display("{}", t),
        }
        NetworkError(t: String) {
            description(""),
            display("{}", t),
        }
        TimeoutError(t: String) {
            description(""),
            display("{}", t),
        }
        InvalidPacketError(t: String) {
            description(""),
            display("{}", t),
        }
    }
}
