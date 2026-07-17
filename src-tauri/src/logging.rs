//! Pipes the app's tracing output to the console page.
//!
//! Everything already logs with `tracing`, so this layer is what makes those lines show
//! up in the UI without the code that writes them knowing the UI exists.

use std::fmt::Write as _;

use tauri::AppHandle;
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::Context;
use tracing_subscriber::Layer;

/// Forwards each log event to the webview.
pub struct WebviewLayer {
    app: AppHandle,
}

impl WebviewLayer {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

impl<S: Subscriber> Layer<S> for WebviewLayer {
    fn on_event(&self, event: &Event<'_>, _context: Context<'_, S>) {
        let mut message = MessageVisitor::default();
        event.record(&mut message);

        if message.text.is_empty() {
            return;
        }

        let level = event.metadata().level().to_string().to_lowercase();
        crate::bridge::emit_log(&self.app, &level, &message.text);
    }
}

/// Pulls the human-readable message out of a tracing event, along with any fields
/// attached to it.
#[derive(Default)]
struct MessageVisitor {
    text: String,
}

impl MessageVisitor {
    fn append(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        // The "message" field is the line itself; everything else is context worth
        // showing after it.
        if field.name() == "message" {
            let formatted = format!("{value:?}");
            self.text.insert_str(0, &trim_debug_quotes(&formatted));
            return;
        }

        let _fits_in_a_string = write!(self.text, " {}={value:?}", field.name());
    }
}

impl Visit for MessageVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.append(field, value);
    }
}

/// A `Debug`-formatted string arrives wrapped in quotes and with its escapes doubled.
/// The console shows text, not Rust literals.
fn trim_debug_quotes(formatted: &str) -> String {
    let unquoted = formatted
        .strip_prefix('"')
        .and_then(|text| text.strip_suffix('"'))
        .unwrap_or(formatted);

    unquoted.replace("\\\"", "\"").replace("\\n", "\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_debug_formatted_message_loses_its_wrapping_quotes() {
        assert_eq!(trim_debug_quotes(r#""hello there""#), "hello there");
    }

    #[test]
    fn escaped_quotes_inside_a_message_are_unescaped() {
        assert_eq!(trim_debug_quotes(r#""heard: \"hi\"""#), r#"heard: "hi""#);
    }

    #[test]
    fn something_that_is_not_a_quoted_string_is_left_alone() {
        assert_eq!(trim_debug_quotes("42"), "42");
    }
}
