// TODO: this regexes need to be improved.
pub static REGEX_COMMAND_FIELD: &str = "command";
pub static REGEX_EXEC_FIELD: &str = "exec";

// otel endpoint regex.
pub static REGEX_OTEL_ENDPOINT: &str = r"\s*endpoint\s*:\s*(.+)";
pub static REGEX_VALID_OTEL_ENDPOINT: &str = r#"^"?(https://)?(staging-otlp\.nr-data\.net|otlp\.nr-data\.net|otlp\.eu01\.nr-data\.net|\$\{OTEL_EXPORTER_OTLP_ENDPOINT\})(:\d+)?"?$"#;
