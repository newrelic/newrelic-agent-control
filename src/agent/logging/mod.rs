use log::SetLoggerError;
pub struct Logging;

impl Logging {
    pub fn init() -> Result<(), SetLoggerError> {
        std_logger::Config::logfmt().try_init()
    }
}
