use log::SetLoggerError;

pub fn init() -> Result<(), SetLoggerError> {
    Ok(std_logger::Config::logfmt().try_init()?)
}
