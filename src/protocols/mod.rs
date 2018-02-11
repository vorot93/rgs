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
    follow_up: Option<models::TProtocol>,
) -> errors::Result<Option<models::TProtocol>> {
    match s {
        "openttds" => Ok(Some(models::TProtocol {
            inner: Arc::new(openttds::Protocol::new(config)?),
        })),
        _ => Ok(None),
    }
}
