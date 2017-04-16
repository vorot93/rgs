pub mod helpers;
pub mod models;

pub mod openttdm;
pub mod openttds;
pub mod q3s;

pub fn get_request_func(s: &str) -> Option<models::RequestFunc> {
    match s {
        "openttds::make_request" => Some(openttds::make_request),
        _ => None,
    }
}
