//! Client behavior for the OpenAI-compatible Chat Completions protocol.

use crate::agent::{AgentMessage, AgentModel, ModelDecision, ToolCall};
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

impl AgentModel for OpenAiCompatibleAgentModel {
    async fn next_decision(&mut self, messages: &[AgentMessage]) -> Result<ModelDecision> {
        let request = build_agent_chat_request(self.model.clone(), self.temperature, messages);

        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let response = self
            .client
            .post(url)
            .bearer_auth(&self.api_key)
            .json(&request)
            .send()
            .await
            .context("failed to send agent request")?;

        let status = response.status();

        let text = response
            .text()
            .await
            .context("failed to read agent response body")?;

        ensure_success_status(status)?;

        parse_agent_decision(&text)
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

            AgentMessage::AssistantToolCalls(tool_calls) => ChatMessage::Assistant {
                content: None,
                tool_calls: tool_calls
                    .iter()
                    .map(|tool_call| ChatToolCall {
                        id: tool_call.id.clone(),
                        kind: "function".to_string(),
                        function: ChatFunctionCall {
                            name: tool_call.name.clone(),
                            arguments: tool_call.input.clone(),
                        },
                    })
                    .collect(),
            },

            AgentMessage::ToolResult {
                tool_call_id,
                output,
                is_error,
                ..
            } => ChatMessage::Tool {
                tool_call_id: tool_call_id.clone(),
                content: if *is_error {
                    serde_json::json!({ "error": output }).to_string()
                } else {
                    output.clone()
                },
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

    if !choice.message.tool_calls.is_empty() {
        let tool_calls = choice
            .message
            .tool_calls
            .iter()
            .map(|tool_call| ToolCall {
                id: tool_call.id.clone(),
                name: tool_call.function.name.clone(),
                input: tool_call.function.arguments.clone(),
            })
            .collect();

        return Ok(ModelDecision::ToolCalls(tool_calls));
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
    use wiremock::matchers::{body_partial_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

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
            AgentMessage::AssistantToolCalls(vec![ToolCall {
                id: "call-1".to_string(),
                name: "add".to_string(),
                input: r#"{"a":2,"b":3}"#.to_string(),
            }]),
            AgentMessage::ToolResult {
                tool_call_id: "call-1".to_string(),
                name: "add".to_string(),
                output: "5".to_string(),
                is_error: false,
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
    fn maps_multiple_tool_calls_to_one_assistant_message() {
        let messages = vec![AgentMessage::AssistantToolCalls(vec![
            ToolCall {
                id: "call-1".to_string(),
                name: "add".to_string(),
                input: r#"{"a":1,"b":2}"#.to_string(),
            },
            ToolCall {
                id: "call-2".to_string(),
                name: "add".to_string(),
                input: r#"{"a":3,"b":4}"#.to_string(),
            },
        ])];

        let value = serde_json::to_value(build_chat_messages(&messages)).unwrap();

        assert_eq!(value.as_array().unwrap().len(), 1);
        assert_eq!(value[0]["role"], "assistant");
        assert_eq!(value[0]["tool_calls"].as_array().unwrap().len(), 2);
        assert_eq!(value[0]["tool_calls"][0]["id"], "call-1");
        assert_eq!(value[0]["tool_calls"][1]["id"], "call-2");
    }

    #[test]
    fn maps_recoverable_tool_error_to_json_content() {
        let messages = vec![AgentMessage::ToolResult {
            tool_call_id: "call-1".to_string(),
            name: "add".to_string(),
            output: "invalid add tool input".to_string(),
            is_error: true,
        }];

        let value = serde_json::to_value(build_chat_messages(&messages)).unwrap();

        assert_eq!(
            value,
            serde_json::json!([{
                "role": "tool",
                "tool_call_id": "call-1",
                "content": r#"{"error":"invalid add tool input"}"#
            }])
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
            ModelDecision::ToolCalls(vec![ToolCall {
                id: "call-1".to_string(),
                name: "add".to_string(),
                input: r#"{"a":2,"b":3}"#.to_string(),
            }])
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
    fn parses_multiple_tool_calls_as_one_decision() {
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

        let decision = parse_agent_decision(&response).unwrap();

        assert_eq!(
            decision,
            ModelDecision::ToolCalls(vec![
                ToolCall {
                    id: "call-1".to_string(),
                    name: "add".to_string(),
                    input: r#"{"a":1,"b":2}"#.to_string(),
                },
                ToolCall {
                    id: "call-2".to_string(),
                    name: "add".to_string(),
                    input: r#"{"a":3,"b":4}"#.to_string(),
                },
            ])
        );
    }

    #[tokio::test]
    async fn next_decision_returns_final_answer() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(header("authorization", "Bearer test-key"))
            .and(body_partial_json(serde_json::json!({
                "model": "test-model",
                "temperature": 0.2,
                "stream": false,
                "messages": [{
                    "role": "user",
                    "content": "hello"
                }]
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{
                    "message": {
                        "content": "done"
                    }
                }]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let mut model = OpenAiCompatibleAgentModel::new(
            server.uri(),
            "test-key".to_string(),
            "test-model".to_string(),
            0.2,
        )
        .unwrap();

        let messages = vec![AgentMessage::User("hello".to_string())];
        let decision = model.next_decision(&messages).await.unwrap();

        assert_eq!(decision, ModelDecision::FinalAnswer("done".to_string()));
    }

    #[tokio::test]
    async fn next_decision_returns_tool_call() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
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
            })))
            .expect(1)
            .mount(&server)
            .await;

        let mut model = OpenAiCompatibleAgentModel::new(
            server.uri(),
            "test-key".to_string(),
            "test-model".to_string(),
            0.2,
        )
        .unwrap();

        let messages = vec![AgentMessage::User("add 2 and 3".to_string())];
        let decision = model.next_decision(&messages).await.unwrap();

        assert_eq!(
            decision,
            ModelDecision::ToolCalls(vec![ToolCall {
                id: "call-1".to_string(),
                name: "add".to_string(),
                input: r#"{"a":2,"b":3}"#.to_string(),
            }])
        );
    }

    #[tokio::test]
    async fn next_decision_rejects_non_success_status() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(401))
            .expect(1)
            .mount(&server)
            .await;

        let mut model = OpenAiCompatibleAgentModel::new(
            server.uri(),
            "test-key".to_string(),
            "test-model".to_string(),
            0.2,
        )
        .unwrap();

        let messages = vec![AgentMessage::User("hello".to_string())];
        let error = model.next_decision(&messages).await.unwrap_err();

        assert_eq!(
            error.to_string(),
            "chat request failed with status 401 Unauthorized"
        );
    }

    #[tokio::test]
    async fn next_decision_rejects_invalid_response_json() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_string("not json"))
            .expect(1)
            .mount(&server)
            .await;

        let mut model = OpenAiCompatibleAgentModel::new(
            server.uri(),
            "test-key".to_string(),
            "test-model".to_string(),
            0.2,
        )
        .unwrap();

        let messages = vec![AgentMessage::User("hello".to_string())];
        let error = model.next_decision(&messages).await.unwrap_err();

        assert!(error.to_string().contains("failed to parse agent response"));
    }
}
