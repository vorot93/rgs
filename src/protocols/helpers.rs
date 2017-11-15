extern crate std;

extern crate handlebars;
extern crate serde_json;

extern crate rgs_models as models;

use errors;
use protocols::models as protocol_models;

pub fn make_request_packet(
    template: &str,
    config: &protocol_models::Config,
) -> errors::Result<Vec<u8>> {
    Ok(
        handlebars::Handlebars::new()
            .template_render(template, config)?
            .into_bytes(),
    )
}
