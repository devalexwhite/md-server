//! Tracing layer that captures log events into a bounded ring buffer for
//! display in the TUI logs panel.

use chrono::Utc;
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};
use tracing::Level;
use tracing_subscriber::Layer;

/// Maximum number of log lines held in the ring buffer.
pub const MAX_LOG_LINES: usize = 200;

/// A single captured log event.
#[derive(Clone)]
pub struct LogEntry {
    pub level: Level,
    pub header: String,
    pub formatted: String,
}

/// A shared, cloneable handle to the rolling log ring-buffer.
pub type LogBuffer = Arc<Mutex<VecDeque<LogEntry>>>;

/// A `tracing_subscriber::Layer` that writes every event into a `LogBuffer`.
pub struct TuiLogLayer {
    buffer: LogBuffer,
}

/// Construct a `TuiLogLayer` and the companion `LogBuffer` the TUI reads from.
///
/// The layer must be registered with the global subscriber before the TUI
/// enters raw mode. The buffer handle is passed into `TuiConfig`.
pub fn new() -> (TuiLogLayer, LogBuffer) {
    let buffer: LogBuffer = Arc::new(Mutex::new(VecDeque::with_capacity(MAX_LOG_LINES)));
    let layer = TuiLogLayer {
        buffer: Arc::clone(&buffer),
    };
    (layer, buffer)
}

impl<S: tracing::Subscriber> Layer<S> for TuiLogLayer {
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let meta = event.metadata();
        let level = *meta.level();
        let target = meta.target();

        let mut message = String::new();
        let mut visitor = MessageVisitor(&mut message);
        event.record(&mut visitor);

        let timestamp = Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ");
        let header = format!("{timestamp} {level} {target}");
        let formatted = format!("{message}");

        let entry = LogEntry {
            level,
            header,
            formatted,
        };

        if let Ok(mut buf) = self.buffer.lock() {
            if buf.len() >= MAX_LOG_LINES {
                buf.pop_front();
            }
            buf.push_back(entry);
        }
        // If the lock is poisoned we silently drop the entry rather than
        // panic in a log handler, which could cause a recursive panic.
    }
}

/// Field visitor that extracts the `message` field from a tracing event.
///
/// `tracing` routes format-string messages through `record_debug`; plain
/// `&'static str` messages come through `record_str`. Both are handled.
struct MessageVisitor<'a>(&'a mut String);

impl tracing::field::Visit for MessageVisitor<'_> {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.0.push_str(value);
        }
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            use std::fmt::Write;
            let before = self.0.len();
            let _ = write!(self.0, "{value:?}");
            // Remove surrounding double-quotes that Debug adds for &str values.
            let written = &self.0[before..];
            if written.starts_with('"') && written.ends_with('"') && written.len() > 1 {
                let unquoted = self.0[before + 1..self.0.len() - 1].to_string();
                self.0.truncate(before);
                self.0.push_str(&unquoted);
            }
        }
    }
}
