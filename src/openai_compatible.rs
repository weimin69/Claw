//! Client behavior for the OpenAI-compatible Chat Completions protocol.

use crate::agent::{AgentMessage, AgentModel, ModelDecision, ToolCall};
use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use std::time::Duration;

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

const DEFAULT_HTTP_TIMEOUT: Duration = Duration::from_secs(60);
const DEFAULT_MAX_ATTEMPTS: usize = 3;
const DEFAULT_INITIAL_RETRY_DELAY: Duration = Duration::from_secs(1);
const DEFAULT_MAX_RETRY_DELAY: Duration = Duration::from_secs(30);

/// Retry policy for model requests. Delays are deterministic so behavior is
/// reproducible; a provider `Retry-After` header overrides the backoff.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RetryPolicy {
    max_attempts: usize,
    initial_delay: Duration,
    max_delay: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: DEFAULT_MAX_ATTEMPTS,
            initial_delay: DEFAULT_INITIAL_RETRY_DELAY,
            max_delay: DEFAULT_MAX_RETRY_DELAY,
        }
    }
}

impl RetryPolicy {
    /// Delay before the next attempt after `failed_attempt`: 1s, 2s, 4s ...,
    /// capped by `max_delay`. A bounded `Retry-After` replaces the backoff.
    fn delay_for(&self, failed_attempt: usize, retry_after: Option<Duration>) -> Duration {
        if let Some(after) = retry_after {
            return after.min(self.max_delay);
        }
        let exponent = failed_attempt.saturating_sub(1).min(16) as u32;
        self.initial_delay
            .saturating_mul(1u32 << exponent)
            .min(self.max_delay)
    }

    #[cfg(test)]
    fn immediate(max_attempts: usize) -> Self {
        Self {
            max_attempts,
            initial_delay: Duration::ZERO,
            max_delay: Duration::ZERO,
        }
    }
}

/// A retry that is about to happen. `attempt` is the attempt that failed.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct RetryNotice {
    pub(crate) attempt: usize,
    pub(crate) max_attempts: usize,
    pub(crate) delay: Duration,
    pub(crate) reason: String,
}

impl RetryNotice {
    pub(crate) fn next_attempt(&self) -> usize {
        self.attempt + 1
    }
}

type RetryObserver = Box<dyn FnMut(&RetryNotice) + Send>;

enum FailureSource {
    Status(reqwest::StatusCode),
    Request(reqwest::Error),
    Body(reqwest::Error),
}

impl FailureSource {
    fn describe(&self) -> String {
        match self {
            Self::Status(status) => format!("status {status}"),
            Self::Request(error) | Self::Body(error) => shorten(&error.to_string(), 160),
        }
    }
}

struct AttemptFailure {
    retryable: bool,
    retry_after: Option<Duration>,
    source: FailureSource,
}

impl AttemptFailure {
    fn reason(&self) -> String {
        self.source.describe()
    }

    fn into_error(self, label: &str, attempts: usize) -> anyhow::Error {
        let suffix = if attempts > 1 {
            format!(" after {attempts} attempts")
        } else {
            String::new()
        };
        match self.source {
            FailureSource::Status(status) => {
                anyhow!("{label} request failed with status {status}{suffix}")
            }
            FailureSource::Request(error) => {
                anyhow::Error::new(error).context(format!("failed to send {label} request{suffix}"))
            }
            FailureSource::Body(error) => anyhow::Error::new(error)
                .context(format!("failed to read {label} response body{suffix}")),
        }
    }
}

struct ChatEndpoint {
    client: reqwest::Client,
    url: String,
    api_key: String,
    retry: RetryPolicy,
    on_retry: Option<RetryObserver>,
}

impl ChatEndpoint {
    fn new(base_url: &str, api_key: String, retry: RetryPolicy) -> Result<Self> {
        Ok(Self {
            client: build_http_client(DEFAULT_HTTP_TIMEOUT)?,
            url: format!("{}/chat/completions", base_url.trim_end_matches('/')),
            api_key,
            retry,
            on_retry: None,
        })
    }

    #[cfg(test)]
    fn with_timeout(mut self, timeout: Duration) -> Result<Self> {
        self.client = build_http_client(timeout)?;
        Ok(self)
    }

    /// Send one chat-completions request, retrying only transient failures.
    /// Authentication, request errors and invalid responses fail immediately.
    async fn complete(&mut self, request: &ChatRequest, label: &str) -> Result<String> {
        let mut attempt = 1;
        loop {
            let failure = match self.send_once(request).await {
                Ok(text) => return Ok(text),
                Err(failure) => failure,
            };

            if !failure.retryable || attempt >= self.retry.max_attempts {
                return Err(failure.into_error(label, attempt));
            }

            let delay = self.retry.delay_for(attempt, failure.retry_after);
            if let Some(observer) = self.on_retry.as_mut() {
                observer(&RetryNotice {
                    attempt,
                    max_attempts: self.retry.max_attempts,
                    delay,
                    reason: failure.reason(),
                });
            }
            if !delay.is_zero() {
                tokio::time::sleep(delay).await;
            }
            attempt += 1;
        }
    }

    async fn send_once(
        &self,
        request: &ChatRequest,
    ) -> std::result::Result<String, AttemptFailure> {
        let response = match self
            .client
            .post(&self.url)
            .bearer_auth(&self.api_key)
            .json(request)
            .send()
            .await
        {
            Ok(response) => response,
            Err(error) => {
                return Err(AttemptFailure {
                    retryable: is_retryable_transport(&error),
                    retry_after: None,
                    source: FailureSource::Request(error),
                });
            }
        };

        let status = response.status();
        if !status.is_success() {
            return Err(AttemptFailure {
                retryable: is_retryable_status(status),
                retry_after: parse_retry_after(response.headers()),
                source: FailureSource::Status(status),
            });
        }

        match response.text().await {
            Ok(text) => Ok(text),
            Err(error) => Err(AttemptFailure {
                retryable: is_retryable_transport(&error),
                retry_after: None,
                source: FailureSource::Body(error),
            }),
        }
    }
}

fn build_http_client(timeout: Duration) -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(timeout)
        .build()
        .context("failed to build HTTP client")
}

fn is_retryable_status(status: reqwest::StatusCode) -> bool {
    matches!(status.as_u16(), 408 | 429 | 500 | 502 | 503 | 504)
}

fn is_retryable_transport(error: &reqwest::Error) -> bool {
    !error.is_builder()
        && (error.is_timeout() || error.is_connect() || error.is_request() || error.is_body())
}

/// Only the delay-seconds form is supported; HTTP-date values are ignored and
/// fall back to the deterministic backoff.
fn parse_retry_after(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    let seconds = headers
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()?;
    Some(Duration::from_secs(seconds))
}

fn shorten(text: &str, max_chars: usize) -> String {
    let mut shortened: String = text.chars().take(max_chars).collect();
    if text.chars().nth(max_chars).is_some() {
        shortened.push('…');
    }
    shortened
}

pub(crate) struct OpenAiCompatibleAgentModel {
    endpoint: ChatEndpoint,
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
        Ok(Self {
            endpoint: ChatEndpoint::new(&base_url, api_key, RetryPolicy::default())?,
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

    pub(crate) fn with_retry_notice<F>(mut self, observer: F) -> Self
    where
        F: FnMut(&RetryNotice) + Send + 'static,
    {
        self.endpoint.on_retry = Some(Box::new(observer));
        self
    }

    #[cfg(test)]
    fn with_retry_policy(mut self, retry: RetryPolicy) -> Self {
        self.endpoint.retry = retry;
        self
    }

    #[cfg(test)]
    fn with_http_timeout(mut self, timeout: Duration) -> Result<Self> {
        self.endpoint = self.endpoint.with_timeout(timeout)?;
        Ok(self)
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

        let text = self.endpoint.complete(&request, "agent").await?;
        parse_agent_decision(&text)
    }
}

pub(crate) async fn send_chat_request<F>(
    base_url: &str,
    api_key: &str,
    model: String,
    temperature: f64,
    prompt: String,
    on_retry: F,
) -> Result<String>
where
    F: FnMut(&RetryNotice) + Send + 'static,
{
    let request = build_chat_request(model, temperature, prompt);
    let mut endpoint = ChatEndpoint::new(base_url, api_key.to_string(), RetryPolicy::default())?;
    endpoint.on_retry = Some(Box::new(on_retry));
    let text = endpoint.complete(&request, "chat").await?;
    parse_chat_response(&text)
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
            description:"Apply 1–100 ordered exact replacements to one existing workspace UTF-8 regular file. Prefer edits: [{old_text,new_text}, ...]; legacy top-level old_text/new_text is also accepted, but never mix formats. Each edit matches the content AFTER previous edits and must have exactly one occurrence, including overlapping matches. Empty old_text is allowed only when the current content is empty. Read the file first. All edits are prepared in memory and saved once only if all succeed; failures identify the edit and leave the file unchanged. Maximum file size is 1 MiB initially and after every edit; no NUL bytes. Returns applied, changed and a bounded diff with diff_truncated. Does not create or delete files. Requires explicit write access; path, permission and stale-content checks still apply.".to_string(),
            parameters:serde_json::json!({
                "type":"object", "properties":{
                    "path":{"type":"string","minLength":1},
                    "old_text":{"type":"string"},
                    "new_text":{"type":"string"},
                    "edits":{"type":"array","minItems":1,"maxItems":100,"items":{
                        "type":"object","properties":{
                            "old_text":{"type":"string"},"new_text":{"type":"string"}
                        },"required":["old_text","new_text"],"additionalProperties":false
                    }}
                },"required":["path"],"additionalProperties":false,
                "oneOf":[
                    {"required":["edits"],"not":{"anyOf":[{"required":["old_text"]},{"required":["new_text"]}]}},
                    {"required":["old_text","new_text"],"not":{"required":["edits"]}}
                ]
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
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
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
    fn classifies_retryable_statuses() {
        for status in [408, 429, 500, 502, 503, 504] {
            assert!(is_retryable_status(
                reqwest::StatusCode::from_u16(status).unwrap()
            ));
        }
        for status in [200, 400, 401, 403, 404, 409, 422] {
            assert!(!is_retryable_status(
                reqwest::StatusCode::from_u16(status).unwrap()
            ));
        }
    }

    #[test]
    fn retry_policy_backs_off_exponentially_with_a_cap() {
        let policy = RetryPolicy::default();

        assert_eq!(policy.delay_for(1, None), Duration::from_secs(1));
        assert_eq!(policy.delay_for(2, None), Duration::from_secs(2));
        assert_eq!(policy.delay_for(3, None), Duration::from_secs(4));
        assert_eq!(policy.delay_for(6, None), Duration::from_secs(30));
        assert_eq!(
            policy.delay_for(1, Some(Duration::from_secs(5))),
            Duration::from_secs(5)
        );
        assert_eq!(
            policy.delay_for(1, Some(Duration::from_secs(120))),
            Duration::from_secs(30)
        );
    }

    #[test]
    fn parses_retry_after_seconds_only() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(reqwest::header::RETRY_AFTER, "2".parse().unwrap());

        assert_eq!(parse_retry_after(&headers), Some(Duration::from_secs(2)));

        let mut date = reqwest::header::HeaderMap::new();
        date.insert(
            reqwest::header::RETRY_AFTER,
            "Wed, 21 Oct 2015 07:28:00 GMT".parse().unwrap(),
        );

        assert_eq!(parse_retry_after(&date), None);
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
    fn builds_agent_chat_request_with_history_and_tools() {
        let messages = vec![AgentMessage::User("read the readme".to_string())];

        let request = build_agent_chat_request("gpt-4.1".to_string(), 0.7, &messages);

        let value = serde_json::to_value(request).unwrap();

        assert_eq!(value["model"], "gpt-4.1");
        assert_eq!(value["messages"][0]["role"], "user");
        assert_eq!(value["messages"][0]["content"], "read the readme");
        assert_eq!(value["tools"][0]["type"], "function");
        assert_eq!(value["tools"][0]["function"]["name"], "list_files");
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
            "agent request failed with status 401 Unauthorized"
        );
    }

    #[tokio::test]
    async fn next_decision_retries_transient_failure_and_reports_notice() {
        let server = MockServer::start().await;
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = calls.clone();
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(move |_request: &wiremock::Request| {
                if seen.fetch_add(1, Ordering::SeqCst) == 0 {
                    ResponseTemplate::new(503)
                } else {
                    ResponseTemplate::new(200).set_body_json(serde_json::json!({
                        "choices": [{"message": {"content": "recovered"}}]
                    }))
                }
            })
            .expect(2)
            .mount(&server)
            .await;

        let notices = Arc::new(Mutex::new(Vec::new()));
        let captured = notices.clone();
        let mut model = OpenAiCompatibleAgentModel::new(
            server.uri(),
            "test-key".to_string(),
            "test-model".to_string(),
            0.2,
        )
        .unwrap()
        .with_retry_policy(RetryPolicy::immediate(3))
        .with_retry_notice(move |notice| {
            captured.lock().unwrap().push((
                notice.attempt,
                notice.next_attempt(),
                notice.max_attempts,
                notice.delay,
                notice.reason.clone(),
            ));
        });

        let decision = model
            .next_decision(&[AgentMessage::User("hello".to_string())])
            .await
            .unwrap();

        assert_eq!(
            decision,
            ModelDecision::FinalAnswer("recovered".to_string())
        );
        assert_eq!(calls.load(Ordering::SeqCst), 2);

        let notices = notices.lock().unwrap();
        assert_eq!(notices.len(), 1);
        assert_eq!((notices[0].0, notices[0].1, notices[0].2), (1, 2, 3));
        assert_eq!(notices[0].3, Duration::ZERO);
        assert!(notices[0].4.contains("503"));
    }

    #[tokio::test]
    async fn next_decision_exhausts_retryable_failures() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(503))
            .expect(3)
            .mount(&server)
            .await;

        let mut model = OpenAiCompatibleAgentModel::new(
            server.uri(),
            "test-key".to_string(),
            "test-model".to_string(),
            0.2,
        )
        .unwrap()
        .with_retry_policy(RetryPolicy::immediate(3));

        let error = model
            .next_decision(&[AgentMessage::User("hello".to_string())])
            .await
            .unwrap_err();
        let message = error.to_string();

        assert!(message.contains("503"), "{message}");
        assert!(message.contains("after 3 attempts"), "{message}");
    }

    #[tokio::test]
    async fn next_decision_does_not_retry_client_errors() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(400))
            .expect(1)
            .mount(&server)
            .await;

        let mut model = OpenAiCompatibleAgentModel::new(
            server.uri(),
            "test-key".to_string(),
            "test-model".to_string(),
            0.2,
        )
        .unwrap()
        .with_retry_policy(RetryPolicy::immediate(3));

        let error = model
            .next_decision(&[AgentMessage::User("hello".to_string())])
            .await
            .unwrap_err();

        assert!(error.to_string().contains("400 Bad Request"), "{error:#}");
    }

    #[tokio::test]
    async fn next_decision_retries_timeout_then_succeeds() {
        let server = MockServer::start().await;
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = calls.clone();
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(move |_request: &wiremock::Request| {
                if seen.fetch_add(1, Ordering::SeqCst) == 0 {
                    ResponseTemplate::new(200)
                        .set_delay(Duration::from_millis(300))
                        .set_body_json(serde_json::json!({
                            "choices": [{"message": {"content": "too late"}}]
                        }))
                } else {
                    ResponseTemplate::new(200).set_body_json(serde_json::json!({
                        "choices": [{"message": {"content": "on time"}}]
                    }))
                }
            })
            .expect(2)
            .mount(&server)
            .await;

        let mut model = OpenAiCompatibleAgentModel::new(
            server.uri(),
            "test-key".to_string(),
            "test-model".to_string(),
            0.2,
        )
        .unwrap()
        .with_retry_policy(RetryPolicy::immediate(2))
        .with_http_timeout(Duration::from_millis(50))
        .unwrap();

        let decision = model
            .next_decision(&[AgentMessage::User("hello".to_string())])
            .await
            .unwrap();

        assert_eq!(decision, ModelDecision::FinalAnswer("on time".to_string()));
        assert_eq!(calls.load(Ordering::SeqCst), 2);
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
