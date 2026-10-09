//! Client behavior for the OpenAI-compatible Chat Completions protocol.

use crate::agent::{AgentMessage, ModelDecision};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    temperature: f64,
    messages: Vec<ChatMessage>,
    stream: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<ChatTool>,
}

#[derive(Serialize)]
#[serde(tag = "role")]
enum ChatMessage {
    #[serde(rename = "user")]
    User { content: String },

    #[serde(rename = "assistant")]
    Assistant {
        content: Option<String>,
        tool_calls: Vec<ChatToolCall>,
    },

    #[serde(rename = "tool")]
    Tool {
        tool_call_id: String,
        content: String,
    },
}

#[derive(Serialize)]
struct ChatTool {
    #[serde(rename = "type")]
    kind: String,
    function: ChatFunctionDefinition,
}

#[derive(Serialize)]
struct ChatFunctionDefinition {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

#[derive(Serialize, Deserialize)]
struct ChatToolCall {
    id: String,

    #[serde(rename = "type")]
    kind: String,

    function: ChatFunctionCall,
}

#[derive(Serialize, Deserialize)]
struct ChatFunctionCall {
    name: String,
    arguments: String,
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
    #[serde(default)]
    tool_calls: Vec<ChatToolCall>,
}

pub(crate) struct OpenAiCompatibleAgentModel {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
    model: String,
    temperature: f64,
}

impl OpenAiCompatibleAgentModel {
    pub(crate) fn new(
        base_url: String,
        api_key: String,
        model: String,
        temperature: f64,
    ) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .context("failed to build HTTP client")?;

        Ok(Self {
            client,
            base_url,
            api_key,
            model,
            temperature,
        })
    }
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

fn build_add_tool() -> ChatTool {
    ChatTool {
        kind: "function".to_string(),
        function: ChatFunctionDefinition {
            name: "add".to_string(),
            description: "Add two integers".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "a": {
                        "type": "integer"
                    },
                    "b": {
                        "type": "integer"
                    }
                },
                "required": ["a", "b"],
                "additionalProperties": false
            }),
        },
    }
}

fn build_chat_messages(messages: &[AgentMessage]) -> Vec<ChatMessage> {
    messages
        .iter()
        .map(|message| match message {
            AgentMessage::User(content) => ChatMessage::User {
                content: content.clone(),
            },

            AgentMessage::ToolCall { id, name, input } => ChatMessage::Assistant {
                content: None,
                tool_calls: vec![ChatToolCall {
                    id: id.clone(),
                    kind: "function".to_string(),
                    function: ChatFunctionCall {
                        name: name.clone(),
                        arguments: input.clone(),
                    },
                }],
            },

            AgentMessage::ToolResult {
                tool_call_id,
                output,
                ..
            } => ChatMessage::Tool {
                tool_call_id: tool_call_id.clone(),
                content: output.clone(),
            },
        })
        .collect()
}

fn build_agent_chat_request(
    model: String,
    temperature: f64,
    messages: &[AgentMessage],
) -> ChatRequest {
    ChatRequest {
        model,
        temperature,
        messages: build_chat_messages(messages),
        stream: false,
        tools: vec![build_add_tool()],
    }
}

fn parse_agent_decision(text: &str) -> Result<ModelDecision> {
    let chat_response: ChatResponse =
        serde_json::from_str(text).context("failed to parse agent response")?;

    let choice = chat_response
        .choices
        .first()
        .context("agent response did not contain any choices")?;

    if choice.message.tool_calls.len() > 1 {
        bail!("multiple tool calls are not supported");
    }

    if let Some(tool_call) = choice.message.tool_calls.first() {
        return Ok(ModelDecision::ToolCall {
            id: tool_call.id.clone(),
            name: tool_call.function.name.clone(),
            input: tool_call.function.arguments.clone(),
        });
    }

    let content = choice
        .message
        .content
        .as_ref()
        .context("agent response did not contain a tool call or final answer")?;

    Ok(ModelDecision::FinalAnswer(content.clone()))
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
        .as_ref()
        .context("chat response choice did not contain message content")?;

    Ok(content.clone())
}

fn build_chat_request(model: String, temperature: f64, prompt: String) -> ChatRequest {
    ChatRequest {
        model,
        temperature,
        messages: vec![ChatMessage::User { content: prompt }],
        stream: false,
        tools: Vec::new(),
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
            r#"{
            "choices": []
            }"#,
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

    #[test]
    fn maps_agent_messages_to_chat_messages() {
        let messages = vec![
            AgentMessage::User("add 2 and 3".to_string()),
            AgentMessage::ToolCall {
                id: "call-1".to_string(),
                name: "add".to_string(),
                input: r#"{"a":2,"b":3}"#.to_string(),
            },
            AgentMessage::ToolResult {
                tool_call_id: "call-1".to_string(),
                name: "add".to_string(),
                output: "5".to_string(),
            },
        ];

        let value = serde_json::to_value(build_chat_messages(&messages)).unwrap();

        assert_eq!(
            value,
            serde_json::json!([
                {
                    "role": "user",
                    "content": "add 2 and 3"
                },
                {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [
                        {
                            "id": "call-1",
                            "type": "function",
                            "function": {
                                "name": "add",
                                "arguments": r#"{"a":2,"b":3}"#
                            }
                        }
                    ]
                },
                {
                    "role": "tool",
                    "tool_call_id": "call-1",
                    "content": "5"
                }
            ])
        );
    }

    #[test]
    fn builds_add_tool_definition() {
        let tool = build_add_tool();
        let value = serde_json::to_value(tool).unwrap();

        assert_eq!(value["type"], "function");
        assert_eq!(value["function"]["name"], "add");
        assert_eq!(value["function"]["parameters"]["type"], "object");

        assert_eq!(
            value["function"]["parameters"]["properties"]["a"]["type"],
            "integer"
        );
        assert_eq!(
            value["function"]["parameters"]["properties"]["b"]["type"],
            "integer"
        );
        assert_eq!(
            value["function"]["parameters"]["required"],
            serde_json::json!(["a", "b"])
        );
        assert_eq!(
            value["function"]["parameters"]["additionalProperties"],
            false
        );
    }

    #[test]
    fn builds_agent_chat_request_with_history_and_tools() {
        let messages = vec![AgentMessage::User("add 2 and 3".to_string())];

        let request = build_agent_chat_request("gpt-4.1".to_string(), 0.7, &messages);

        let value = serde_json::to_value(request).unwrap();

        assert_eq!(value["model"], "gpt-4.1");
        assert_eq!(value["messages"][0]["role"], "user");
        assert_eq!(value["messages"][0]["content"], "add 2 and 3");
        assert_eq!(value["tools"][0]["type"], "function");
        assert_eq!(value["tools"][0]["function"]["name"], "add");
    }

    #[test]
    fn parses_tool_call_as_agent_decision() {
        let decision = parse_agent_decision(
            r#"{
                "choices": [{
                    "message": {
                        "content": null,
                        "tool_calls": [{
                            "id": "call-1",
                            "type": "function",
                            "function": {
                                "name": "add",
                                "arguments": "{\"a\":2,\"b\":3}"
                            }
                        }]
                    }
                }]
            }"#,
        )
        .unwrap();

        assert_eq!(
            decision,
            ModelDecision::ToolCall {
                id: "call-1".to_string(),
                name: "add".to_string(),
                input: r#"{"a":2,"b":3}"#.to_string(),
            }
        );
    }

    #[test]
    fn parses_final_answer_as_agent_decision() {
        let decision = parse_agent_decision(
            r#"{
                "choices": [{
                    "message": {
                        "content": "done"
                    }
                }]
            }"#,
        )
        .unwrap();

        assert_eq!(decision, ModelDecision::FinalAnswer("done".to_string()));
    }

    #[test]
    fn rejects_multiple_tool_calls() {
        let response = serde_json::json!({
            "choices": [{
                "message": {
                    "content": null,
                    "tool_calls": [
                        {
                            "id": "call-1",
                            "type": "function",
                            "function": {
                                "name": "add",
                                "arguments": "{\"a\":1,\"b\":2}"
                            }
                        },
                        {
                            "id": "call-2",
                            "type": "function",
                            "function": {
                                "name": "add",
                                "arguments": "{\"a\":3,\"b\":4}"
                            }
                        }
                    ]
                }
            }]
        })
        .to_string();

        let error = parse_agent_decision(&response).unwrap_err();

        assert_eq!(error.to_string(), "multiple tool calls are not supported");
    }
}
