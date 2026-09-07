use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    temperature: f64,
    messages: Vec<ChatMessage>,
    stream: bool,
}

#[derive(Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: AssistantMessage,
}

#[derive(Deserialize)]
struct AssistantMessage {
    content: Option<String>,
}

pub async fn send_chat_request(
    base_url: &str,
    api_key: &str,
    model: String,
    temperature: f64,
    prompt: String,
) -> Result<String> {
    let request = build_chat_request(model, temperature, prompt);
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .context("failed to build HTTP client")?;

    let response = client
        .post(url)
        .bearer_auth(api_key)
        .json(&request)
        .send()
        .await
        .context("failed to send chat request")?;

    let status = response.status();
    let text = response
        .text()
        .await
        .context("failed to read chat response body")?;

    ensure_success_status(status)?;

    parse_chat_response(&text)
}

fn ensure_success_status(status: reqwest::StatusCode) -> Result<()> {
    if !status.is_success() {
        bail!("chat request failed with status {}", status);
    }

    Ok(())
}

fn parse_chat_response(text: &str) -> Result<String> {
    let chat_response: ChatResponse =
        serde_json::from_str(text).context("failed to parse chat response")?;

    let choice = chat_response
        .choices
        .first()
        .context("chat response did not contain any choices")?;

    let content = choice
        .message
        .content
        .clone()
        .context("chat response choice did not contain message content")?;

    Ok(content)
}

fn build_chat_request(model: String, temperature: f64, prompt: String) -> ChatRequest {
    ChatRequest {
        model,
        temperature,
        messages: vec![ChatMessage {
            role: "user".to_string(),
            content: prompt,
        }],
        stream: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_chat_request() {
        let request = build_chat_request("gpt-4.1".to_string(), 0.7, "hello rust".to_string());

        let value = serde_json::to_value(&request).unwrap();

        assert_eq!(value["model"], "gpt-4.1");
        assert_eq!(value["temperature"], 0.7);
        assert_eq!(value["stream"], false);
        assert_eq!(value["messages"][0]["role"], "user");
        assert_eq!(value["messages"][0]["content"], "hello rust");
    }

    #[test]
    fn parses_chat_response_content() {
        let content = parse_chat_response(
            r#"{
                  "choices": [
                      {
                          "message": {
                              "role": "assistant",
                              "content": "Hello from the assistant"
                          }
                      }
                  ]
              }"#,
        )
        .unwrap();

        assert_eq!(content, "Hello from the assistant");
    }

    #[test]
    fn rejects_chat_response_without_choices() {
        let error = parse_chat_response(
            r#"{ "choices":
            [] }"#,
        )
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("chat response did not contain any choices")
        );
    }
    #[test]
    fn rejects_chat_response_without_content() {
        let error = parse_chat_response(
            r#"{
                     "choices": [
                         {
                             "message": {
                                 "role": "assistant",
                                 "content": null
                             }
                         }
                     ]
                 }"#,
        )
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("chat response choice did not contain message content")
        );
    }

    #[test]
    fn rejects_invalid_chat_response_json() {
        let error = parse_chat_response("not json").unwrap_err();

        assert!(error.to_string().contains("failed to parse chat response"));
    }

    #[test]
    fn accepts_success_status() {
        let result = ensure_success_status(reqwest::StatusCode::OK);

        assert!(result.is_ok());
    }

    #[test]
    fn rejects_error_status() {
        let error = ensure_success_status(reqwest::StatusCode::UNAUTHORIZED).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("chat request failed with status 401 Unauthorized")
        );
    }
}
