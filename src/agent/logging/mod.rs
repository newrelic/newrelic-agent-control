use log::SetLoggerError;

pub fn init() -> Result<(), SetLoggerError> {
    std_logger::Config::logfmt().try_init()
}
