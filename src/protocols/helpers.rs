use errors;
use errors::Error;
use protocols::models as protocol_models;

use handlebars;
use std;

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
