use std::fmt::{self, Debug};
use tracing::{
    field::{Field, Visit},
    Event, Subscriber,
};
use tracing_subscriber::{
    fmt::{format::Writer, FmtContext, FormatEvent, FormatFields},
    registry::LookupSpan,
};

pub(crate) struct SubAgentFileLogger;

impl<S, N> FormatEvent<S, N> for SubAgentFileLogger
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        _ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        // We are interested in the field called "file_log_line", we ignore otherwise
        event.fields().try_for_each(|field| {
            if field.name() == "file_log_line" {
                let mut visitor = LogLineVisitor::default();
                event.record(&mut visitor);
                writeln!(writer, "{}", visitor.0)
            } else {
                Ok(())
            }
        })
    }
}

#[derive(Default, Debug)]
struct LogLineVisitor(String);

impl Visit for LogLineVisitor {
    // Required but we do not expect it
    fn record_debug(&mut self, _field: &Field, value: &dyn Debug) {
        // Store the value in the visitor
        self.0 = format!("{:?}", value);
    }

    fn record_str(&mut self, _field: &Field, value: &str) {
        // Store the value in the visitor
        self.0 = value.to_owned();
    }
}
