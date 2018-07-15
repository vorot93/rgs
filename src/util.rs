pub trait LoggingService {
    fn debug(&self, msg: &str);
    fn info(&self, msg: &str);
}

#[derive(Clone)]
pub struct RealLogger;

impl LoggingService for RealLogger {
    fn debug(&self, msg: &str) {
        println!("DEBUG: {}", msg)
    }

    fn info(&self, msg: &str) {
        println!("INFO: {}", msg)
    }
}
