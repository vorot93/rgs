extern crate std;

extern crate handlebars;
extern crate serde_json;

extern crate rgs_models as models;

use errors;
use errors::Error;
use protocols::models as protocol_models;

pub fn make_request_packet(
    template: &str,
    config: &protocol_models::Config,
) -> errors::Result<Vec<u8>> {
    Ok(handlebars::Handlebars::new()
        .render_template(template, config)
        .map_err(|e| Error::IOError {
            reason: std::error::Error::description(&e).into(),
        })?
        .into_bytes())
}
