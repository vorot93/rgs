extern crate std;

extern crate handlebars;
extern crate serde_json;

extern crate rgs_models as models;

use errors::Error;
use protocols::models as protocol_models;

pub fn make_request_packet(
    template: &str,
    config: &protocol_models::Config,
) -> Result<Vec<u8>, Error> {
    let s = try!(handlebars::Handlebars::new().template_render(
        template,
        config,
    ));
    Ok(s.into_bytes())
}
