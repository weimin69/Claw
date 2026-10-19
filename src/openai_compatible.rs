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
        #[serde(skip_serializing_if = "Vec::is_empty")]
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
    allow_write: bool,
    allow_exec: bool,
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
            allow_write: false,
            allow_exec: false,
        })
    }

    pub(crate) fn with_write_access(mut self, allow_write: bool) -> Self {
        self.allow_write = allow_write;
        self
    }

    pub(crate) fn with_exec_access(mut self, allow_exec: bool) -> Self {
        self.allow_exec = allow_exec;
        self
    }
}

impl AgentModel for OpenAiCompatibleAgentModel {
    async fn next_decision(&mut self, messages: &[AgentMessage]) -> Result<ModelDecision> {
        let mut request = build_agent_chat_request(self.model.clone(), self.temperature, messages);
        if self.allow_write {
            request.tools.push(build_replace_text_tool());
            request.tools.push(build_create_file_tool());
        }
        if self.allow_exec {
            request.tools.push(build_run_command_tool());
        }

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

fn build_list_files_tool() -> ChatTool {
    ChatTool {
        kind: "function".to_string(),
        function: ChatFunctionDefinition {
            name: "list_files".to_string(),
            description: "List direct children of a workspace directory, sorted by path. Returns at most 200 entries and a truncated flag. Paths are workspace-relative; use '.' for the root. Symlinks are listed without following them.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {"path": {"type": "string", "minLength": 1}},
                "required": ["path"],
                "additionalProperties": false
            }),
        },
    }
}

fn build_read_file_tool() -> ChatTool {
    ChatTool {
        kind: "function".to_string(),
        function: ChatFunctionDefinition {
            name: "read_file".to_string(),
            description: "Read a workspace UTF-8 regular file up to 1 MiB. Lines are 1-based and inclusive; defaults to 200 lines from start_line (default 1). Returns at most 500 lines and 32 KiB of content. truncated means content remains after the returned text, including when end_line was requested; the last line may be partial at the byte limit. Empty files return end_line 0.".to_string(),
            parameters: serde_json::json!({
                "type":"object",
                "properties": {
                    "path":{"type":"string","minLength":1},
                    "start_line":{"type":"integer","minimum":1},
                    "end_line":{"type":"integer","minimum":1}
                },
                "required":["path"], "additionalProperties":false
            }),
        },
    }
}

fn build_search_code_tool() -> ChatTool {
    ChatTool {
        kind: "function".to_string(),
        function: ChatFunctionDefinition {
            name: "search_code".to_string(),
            description: "Search a workspace file or directory recursively for a case-sensitive literal single-line pattern (1-1024 bytes). Skip discovered symlinks, .git and target directories, binary/non-UTF-8 files, and files over 1 MiB. Scan at most 5000 directory entries and 8 MiB. Return up to 100 matching lines sorted by path and line, within 32 KiB JSON. Each text excerpt is at most 1024 bytes and may start at the match; line_truncated marks shortened lines. truncated marks incomplete scanning/results; skipped_files counts unsupported files. No matches is a successful result. Does not apply .gitignore rules.".to_string(),
            parameters: serde_json::json!({
                "type":"object",
                "properties": {
                    "pattern":{"type":"string","minLength":1,"maxLength":1024},
                    "path":{"type":"string","minLength":1}
                },
                "required":["pattern","path"], "additionalProperties":false
            }),
        },
    }
}

fn build_run_command_tool() -> ChatTool {
    ChatTool {
        kind:"function".to_string(),
        function:ChatFunctionDefinition {
            name:"run_command".to_string(),
            description:"Execute a program directly with an argument array (no implicit shell). cwd defaults to '.' and must be an existing workspace directory. timeout_seconds defaults to 60, allowed 1-300. stdout and stderr are independently capped at 32 KiB and drained concurrently. Returns success, exit_code, timed_out, stdout, stderr, truncation flags and duration_ms. Nonzero status and timeout are recoverable tool failures. stdin is closed. Unix processes run in a separate group, killed on timeout or cancellation; descendants can escape that group. Requires --allow-exec: arbitrary execution can write files and access the host beyond the workspace, independently of --allow-write. Not an OS sandbox.".to_string(),
            parameters:serde_json::json!({
                "type":"object","properties":{
                    "program":{"type":"string","minLength":1},
                    "args":{"type":"array","items":{"type":"string"},"maxItems":128},
                    "cwd":{"type":"string","minLength":1},
                    "timeout_seconds":{"type":"integer","minimum":1,"maximum":300}
                },"required":["program","args"],"additionalProperties":false
            }),
        },
    }
}

fn build_create_file_tool() -> ChatTool {
    ChatTool {
        kind:"function".to_string(),
        function:ChatFunctionDefinition {
            name:"create_file".to_string(),
            description:"Create a new workspace UTF-8 file with initial content (maximum 1 MiB, no NUL bytes). Parent directory must already exist inside the workspace. Never overwrites an existing file, directory or symlink. Empty content creates an empty file. Returns path, created and bytes_written. Requires explicit write access. Use replace_text for existing files.".to_string(),
            parameters:serde_json::json!({
                "type":"object","properties":{
                    "path":{"type":"string","minLength":1},
                    "content":{"type":"string"}
                },"required":["path","content"],"additionalProperties":false
            }),
        },
    }
}

fn build_replace_text_tool() -> ChatTool {
    ChatTool {
        kind: "function".to_string(),
        function: ChatFunctionDefinition {
            name:"replace_text".to_string(),
            description:"Replace one unique exact occurrence in an existing workspace UTF-8 regular file (maximum 1 MiB before and after). Empty old_text is allowed only when the file is empty, to initialize it with new_text. For nonempty files old_text must be nonempty and uniquely identify the edit, including overlapping occurrences. Preserve other content. Use read_file first. Returns applied, changed and a diff; diff_truncated marks an incomplete diff. Does not create or delete files. Conflicting content, read-only files and invalid paths return errors without writing. This tool is available only with explicit write access.".to_string(),
            parameters:serde_json::json!({
                "type":"object", "properties":{
                    "path":{"type":"string","minLength":1},
                    "old_text":{"type":"string"},
                    "new_text":{"type":"string"}
                },"required":["path","old_text","new_text"],"additionalProperties":false
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
            AgentMessage::Assistant(content) => ChatMessage::Assistant {
                content: Some(content.clone()),
                tool_calls: Vec::new(),
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
        tools: vec![
            build_add_tool(),
            build_list_files_tool(),
            build_read_file_tool(),
            build_search_code_tool(),
        ],
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
