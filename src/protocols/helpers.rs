use failure;
use models::*;

use handlebars;

pub fn make_request_packet(template: &str, config: &Config) -> Result<Vec<u8>, failure::Error> {
    Ok(handlebars::Handlebars::new()
        .render_template(template, config)?
        .into_bytes())
}
