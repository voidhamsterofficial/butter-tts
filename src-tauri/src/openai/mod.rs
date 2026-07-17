//! The two OpenAI calls the pipeline makes: transcribe an utterance, speak a line.

pub mod stt;
pub mod tts;

use std::time::Duration;

const OPENAI_API_BASE: &str = "https://api.openai.com/v1";

/// Generous enough for a slow transcription of a long utterance, short enough that a
/// hung request eventually gives the console something to report.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, thiserror::Error)]
pub enum OpenAiError {
    #[error("could not build an HTTP client: {0}")]
    Client(#[source] reqwest::Error),

    #[error("could not reach the OpenAI API: {0}")]
    Transport(#[source] reqwest::Error),

    #[error("OpenAI rejected the request ({status}): {message}")]
    Api { status: u16, message: String },
}

/// A configured caller for the OpenAI endpoints, holding the API key and a pooled
/// HTTP client. Cheap to clone.
#[derive(Debug, Clone)]
pub struct OpenAiClient {
    http_client: reqwest::Client,
    api_key: String,
}

impl OpenAiClient {
    pub fn new(api_key: &str) -> Result<Self, OpenAiError> {
        let http_client = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(OpenAiError::Client)?;

        Ok(Self {
            http_client,
            api_key: api_key.trim().to_string(),
        })
    }

    fn endpoint(&self, path: &str) -> String {
        format!("{OPENAI_API_BASE}{path}")
    }
}

/// Turns a non-success response into an [`OpenAiError::Api`] carrying whatever the API
/// said, so a bad key or a rate limit reaches the console as a readable line rather
/// than a bare status code.
async fn error_for_status(response: reqwest::Response) -> Result<reqwest::Response, OpenAiError> {
    let status = response.status();

    if status.is_success() {
        return Ok(response);
    }

    let message = response
        .text()
        .await
        .unwrap_or_else(|_| "no response body".to_string());

    Err(OpenAiError::Api {
        status: status.as_u16(),
        message: extract_api_message(&message),
    })
}

/// Digs the human-readable message out of OpenAI's error envelope, falling back to the
/// raw body when it is not the shape we expect.
fn extract_api_message(body: &str) -> String {
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(body) else {
        return body.trim().to_string();
    };

    let message = parsed
        .get("error")
        .and_then(|error| error.get("message"))
        .and_then(|message| message.as_str());

    match message {
        Some(message) => message.to_string(),
        None => body.trim().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_api_error_envelope_is_reduced_to_its_message() {
        let body =
            r#"{"error":{"message":"Incorrect API key provided.","type":"invalid_request_error"}}"#;

        assert_eq!(extract_api_message(body), "Incorrect API key provided.");
    }

    #[test]
    fn an_unexpected_body_is_passed_through_rather_than_swallowed() {
        let body = "502 Bad Gateway";

        assert_eq!(extract_api_message(body), "502 Bad Gateway");
    }

    #[test]
    fn a_key_is_trimmed_so_a_pasted_newline_does_not_break_the_header() {
        let client = OpenAiClient::new("  sk-test\n").expect("client should build");

        assert_eq!(client.api_key, "sk-test");
    }
}
