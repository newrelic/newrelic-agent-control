use std::error::Error;

/// Type alias to represent test results.
pub type TestResult<T> = Result<T, Box<dyn Error>>;
