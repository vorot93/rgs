pub mod helpers;
pub mod models;

pub mod openttdm;
pub mod openttds;
pub mod q3s;

use errors;

use std::sync::Arc;

pub fn make_protocol(
    s: &str,
    config: &models::Config,
    follow_up: Option<Arc<models::Protocol>>,
) -> errors::Result<Option<Arc<models::Protocol>>> {
    match s {
        "openttds" => Ok(Some(Arc::new(openttds::Protocol::new(config)?))),
        _ => Ok(None),
    }
}
