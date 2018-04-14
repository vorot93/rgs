use errors;
use errors::Error;
use models::*;

use handlebars;
use std;

pub fn make_request_packet(template: &str, config: &Config) -> errors::Result<Vec<u8>> {
    Ok(handlebars::Handlebars::new()
        .render_template(template, config)
        .map_err(|e| Error::IOError {
            reason: std::error::Error::description(&e).into(),
        })?
        .into_bytes())
}
