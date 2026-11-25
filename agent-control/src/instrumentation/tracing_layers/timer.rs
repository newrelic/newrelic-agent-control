//! Local-time timer for log timestamps, formatted with a user-provided strftime pattern.

use std::fmt::Result as FmtResult;
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::time::FormatTime;

/// Formats the current local time using a strftime pattern.
#[derive(Debug, Clone)]
pub struct LocalTimer {
    format: String,
}

impl LocalTimer {
    pub fn new(format: impl Into<String>) -> Self {
        Self {
            format: format.into(),
        }
    }
}

impl FormatTime for LocalTimer {
    fn format_time(&self, w: &mut Writer<'_>) -> FmtResult {
        write!(w, "{}", jiff::Zoned::now().strftime(&self.format))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn render(format: &str) -> Result<String, std::fmt::Error> {
        let mut out = String::new();
        LocalTimer::new(format).format_time(&mut Writer::new(&mut out))?;
        Ok(out)
    }

    #[rstest]
    #[case::default_format("%Y-%m-%dT%H:%M:%S", 19)]
    #[case::millis("%Y-%m-%dT%H:%M:%S%.3f", 23)]
    #[case::date_only("%Y-%m-%d", 10)]
    fn formats_current_time(#[case] format: &str, #[case] expected_len: usize) {
        assert_eq!(render(format).unwrap().len(), expected_len);
    }

    #[test]
    fn literal_text_is_preserved() {
        assert_eq!(render("custom_format").unwrap(), "custom_format");
    }

    #[test]
    fn trailing_percent_is_kept_as_literal() {
        assert_eq!(render("abc%").unwrap(), "abc%");
    }
}
