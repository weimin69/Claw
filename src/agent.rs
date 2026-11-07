use anyhow::{Context, Ok, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum AgentEvent {
    ContextTrimmed {
        turns: usize,
    },
    ModelStarted {
        iteration: usize,
    },
    ModelFinished {
        iteration: usize,
        elapsed: Duration,
        is_error: bool,
    },
    ToolStarted {
        name: String,
    },
    ToolFinished {
        name: String,
        elapsed: Duration,
        is_error: bool,
    },
}

/// A fact reported by a tool about something that actually happened.
/// Effects are never sent to the model and never persisted.
#[derive(Debug, PartialEq, Eq, Clone)]
pub(crate) enum ToolEffect {
    CreatedFile {
        path: String,
    },
    ModifiedFile {
        path: String,
        edits: usize,
    },
    CommandFinished {
        program: String,
        args: Vec<String>,
        exit_code: Option<i32>,
        timed_out: bool,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct AgentRunResult {
    pub(crate) answer: String,
    pub(crate) iterations: usize,
    pub(crate) tool_calls: BTreeMap<String, usize>,
    /// Ordered, possibly repeated facts from this turn's successful writes and
    /// executed commands. Presentation-level aggregation happens in the CLI.
    pub(crate) effects: Vec<ToolEffect>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ModelDecision {
    FinalAnswer(String),
    ToolCalls(Vec<ToolCall>),
}

#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
pub(crate) struct ToolCall {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) input: String,
}

#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub(crate) enum AgentMessage {
    User(String),
    Assistant(String),
    AssistantToolCalls(Vec<ToolCall>),
    ToolResult {
        tool_call_id: String,
        name: String,
        output: String,
        is_error: bool,
    },
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub(crate) struct ToolOutput {
    pub(crate) content: String,
    pub(crate) is_error: bool,
    pub(crate) effects: Vec<ToolEffect>,
}

impl ToolOutput {
    pub(crate) fn success(content: String) -> Self {
        Self {
            content,
            is_error: false,
            effects: Vec::new(),
        }
    }

    pub(crate) fn error(content: String) -> Self {
        Self {
            content,
            is_error: true,
            effects: Vec::new(),
        }
    }

    pub(crate) fn with_effects(mut self, effects: Vec<ToolEffect>) -> Self {
        self.effects = effects;
        self
    }
}

pub(crate) trait AgentModel {
    async fn next_decision(&mut self, messages: &[AgentMessage]) -> Result<ModelDecision>;
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct AgentState {
    messages: Vec<AgentMessage>,
    iteration: usize,
    max_iterations: Option<usize>,
    tool_calls: usize,
    max_tool_calls: Option<usize>,
}

impl AgentState {
    /// Record new events in both the model context and the unabridged history.
    /// Existing context is never copied back into the history at turn completion.
    fn record_message(&mut self, message: AgentMessage, full_history: &mut Vec<AgentMessage>) {
        full_history.push(message.clone());
        self.messages.push(message);
    }

    pub(crate) fn new(
        prompt: String,
        max_iterations: Option<usize>,
        max_tool_calls: Option<usize>,
    ) -> Result<Self> {
        if prompt.trim().is_empty() {
            bail!("agent prompt cannot be empty");
        }

        if max_tool_calls == Some(0) {
            bail!("max_tool_calls must be greater than 0");
        }

        if max_iterations == Some(0) {
            bail!("max_iterations must be greater than 0");
        }

        Ok(Self {
            messages: vec![AgentMessage::User(prompt)],
            iteration: 0,
            max_iterations,
            tool_calls: 0,
            max_tool_calls,
        })
    }

    fn begin_tool_batch(&mut self, count: usize) -> Result<()> {
        let next = self
            .tool_calls
            .checked_add(count)
            .ok_or_else(|| invariant(anyhow::anyhow!("tool call count overflow")))?;

        if self.max_tool_calls.is_some_and(|limit| next > limit) {
            bail!("agent reached maximum tool calls");
        }

        self.tool_calls = next;
        Ok(())
    }

    fn begin_iteration(&mut self) -> Result<()> {
        if self
            .max_iterations
            .is_some_and(|limit| self.iteration >= limit)
        {
            bail!("agent reached maximum iterations");
        }
        self.iteration = self
            .iteration
            .checked_add(1)
            .ok_or_else(|| invariant(anyhow::anyhow!("iteration count overflow")))?;
        Ok(())
    }
}

/// In-memory history. A failed or interrupted turn rolls the conversation back
/// to the last completed turn. Only an internal invariant violation closes the
/// session, because the history can no longer be trusted then.
pub(crate) struct AgentSession {
    /// Original messages for persistence and audit; never replaced by a summary.
    full_history: Vec<AgentMessage>,
    /// The messages sent to the model. Temporarily owned by AgentState during a turn.
    working_context: Option<Vec<AgentMessage>>,
    /// Snapshot used to roll back an in-flight turn, including one whose future
    /// was dropped by cancellation.
    pending_turn: Option<TurnCheckpoint>,
    closed: bool,
    max_context_bytes: usize,
}

struct TurnCheckpoint {
    full_history_len: usize,
    working_context: Option<Vec<AgentMessage>>,
}

pub(crate) const DEFAULT_MAX_CONTEXT_BYTES: usize = 256 * 1024;

/// Where the context size was checked. The CLI reports the two cases
/// differently, and only `Input` means the request was never sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContextLimitStage {
    Input,
    TurnInProgress,
}

#[derive(Debug)]
pub(crate) struct ContextLimitExceeded {
    pub(crate) actual: usize,
    pub(crate) limit: usize,
    pub(crate) stage: ContextLimitStage,
}

/// Marks failures that leave the in-memory session unsafe to reuse. Every other
/// turn failure is recoverable: the conversation is rolled back and the session
/// stays open.
#[derive(Debug)]
struct SessionInvariant(anyhow::Error);

impl std::fmt::Display for SessionInvariant {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

impl std::error::Error for SessionInvariant {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.0.as_ref())
    }
}

fn invariant(error: anyhow::Error) -> anyhow::Error {
    anyhow::Error::new(SessionInvariant(error))
}

impl std::fmt::Display for ContextLimitExceeded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "agent context exceeds size limit: {} bytes, limit {} bytes (internal message JSON, not tokens)",
            self.actual, self.limit
        )
    }
}
impl std::error::Error for ContextLimitExceeded {}

impl Default for AgentSession {
    fn default() -> Self {
        Self {
            full_history: Vec::new(),
            working_context: None,
            pending_turn: None,
            closed: false,
            max_context_bytes: DEFAULT_MAX_CONTEXT_BYTES,
        }
    }
}

impl AgentSession {
    pub(crate) fn context_limit(&self) -> usize {
        self.max_context_bytes
    }
    pub(crate) fn new_conversation(&self) -> Self {
        Self {
            max_context_bytes: self.max_context_bytes,
            ..Self::default()
        }
    }
    pub(crate) fn from_parts(
        full_history: Vec<AgentMessage>,
        working_context: Vec<AgentMessage>,
    ) -> Result<Self> {
        validate_history(&working_context).context("invalid working context")?;
        let mut session = Self::from_history(full_history)?;
        session.working_context = Some(working_context);
        Ok(session)
    }

    pub(crate) fn completed_context(&self) -> Result<&[AgentMessage]> {
        self.completed_messages()?;
        self.working_context
            .as_deref()
            .context("session has no completed working context")
    }
    pub(crate) fn set_context_limit(&mut self, limit: usize) -> Result<()> {
        if limit == 0 {
            bail!("max_context_bytes must be greater than 0");
        }
        self.max_context_bytes = limit;
        Ok(())
    }

    pub(crate) fn can_continue(&self) -> bool {
        !self.closed
    }
    pub(crate) fn from_history(messages: Vec<AgentMessage>) -> Result<Self> {
        validate_history(&messages)?;
        Ok(Self {
            working_context: Some(messages.clone()),
            full_history: messages,
            pending_turn: None,
            closed: false,
            max_context_bytes: DEFAULT_MAX_CONTEXT_BYTES,
        })
    }
    /// Only completed turns may be persisted; never expose partial tool batches.
    pub(crate) fn completed_messages(&self) -> Result<&[AgentMessage]> {
        if self.closed {
            bail!("cannot save an interrupted or failed session");
        }
        if self.full_history.is_empty() {
            bail!("session has no completed turn to save");
        }
        Ok(&self.full_history)
    }

    pub(crate) async fn run_turn<M: AgentModel, T: ToolExecutor>(
        &mut self,
        model: &mut M,
        prompt: String,
        max_iterations: Option<usize>,
        max_tool_calls: Option<usize>,
        tool_executor: &mut T,
        mut on_event: impl FnMut(AgentEvent),
    ) -> Result<AgentRunResult> {
        if self.closed {
            bail!("agent session is closed after an interrupted or failed turn");
        }
        if self.pending_turn.is_some() {
            bail!("agent session already has an unfinished turn");
        }
        let mut state = AgentState::new(prompt, max_iterations, max_tool_calls)?;
        // Snapshot before handing the working context to AgentState. A dropped
        // future (cancellation) cannot run cleanup inside this function, so the
        // snapshot lives on the session and `abort_turn` restores it later.
        let checkpoint = TurnCheckpoint {
            full_history_len: self.full_history.len(),
            working_context: self.working_context.take(),
        };
        let mut messages = checkpoint.working_context.clone().unwrap_or_default();
        self.pending_turn = Some(checkpoint);
        let previous_len = messages.len();
        messages.append(&mut state.messages);
        state.messages = messages;
        let start = match context_start(&state.messages, self.max_context_bytes) {
            std::result::Result::Ok(start) => start,
            Err(error) => return Err(self.fail_turn(error)),
        };
        // The input is accepted only after preflight succeeds. Do not append
        // rejected inputs, nor old working messages (which may later be summaries).
        self.full_history.push(state.messages[previous_len].clone());
        let removed_turns = state.messages[..start]
            .iter()
            .filter(|message| matches!(message, AgentMessage::Assistant(_)))
            .count();
        state.messages.drain(..start);
        self.closed = true;
        if removed_turns > 0 {
            on_event(AgentEvent::ContextTrimmed {
                turns: removed_turns,
            });
        }
        let result = match run_turn_loop(
            model,
            &mut state,
            tool_executor,
            on_event,
            self.max_context_bytes,
            &mut self.full_history,
        )
        .await
        {
            std::result::Result::Ok(result) => result,
            Err(error) => return Err(self.fail_turn(error)),
        };
        self.pending_turn = None;
        self.working_context = Some(state.messages);
        self.closed = false;
        Ok(result)
    }

    /// Record a failed turn: always roll the conversation back, and close the
    /// session only when the failure is an internal invariant violation.
    fn fail_turn(&mut self, error: anyhow::Error) -> anyhow::Error {
        let fatal = error.is::<SessionInvariant>();
        self.rollback_turn();
        if fatal {
            self.closed = true;
        }
        error
    }

    /// Restore the conversation to the state before the in-flight turn. This
    /// never touches files, commands or saved archives.
    fn rollback_turn(&mut self) {
        if let Some(checkpoint) = self.pending_turn.take() {
            self.full_history.truncate(checkpoint.full_history_len);
            self.working_context = checkpoint.working_context;
            self.closed = false;
        }
    }

    /// Restore the conversation after a cancelled turn. Idempotent, and safe to
    /// call when no turn is in flight.
    pub(crate) fn abort_turn(&mut self) {
        self.rollback_turn();
    }
}

/// Accept only complete turns in the same order produced by the runtime.
pub(crate) fn validate_history(messages: &[AgentMessage]) -> Result<()> {
    let mut expect_user = true;
    let mut pending: std::collections::VecDeque<(&str, &str)> = Default::default();
    if messages.is_empty() {
        bail!("session history is empty");
    }
    for message in messages {
        match message {
            AgentMessage::User(text)
                if expect_user && pending.is_empty() && !text.trim().is_empty() =>
            {
                expect_user = false;
            }
            AgentMessage::Assistant(_) if !expect_user && pending.is_empty() => {
                expect_user = true;
            }
            AgentMessage::AssistantToolCalls(calls) if !expect_user && pending.is_empty() => {
                if calls.is_empty() {
                    bail!("session contains an empty tool call batch");
                }
                let mut ids = std::collections::HashSet::new();
                for call in calls {
                    if call.id.trim().is_empty()
                        || call.name.trim().is_empty()
                        || !ids.insert(&call.id)
                    {
                        bail!("session contains invalid or duplicate tool call IDs/names");
                    }
                    pending.push_back((&call.id, &call.name));
                }
            }
            AgentMessage::ToolResult {
                tool_call_id, name, ..
            } if !expect_user => {
                if pending.pop_front() != Some((tool_call_id.as_str(), name.as_str())) {
                    bail!("session tool result does not match the pending tool call");
                }
            }
            _ => bail!("invalid session message order"),
        }
    }
    if !expect_user || !pending.is_empty() {
        bail!("session contains an incomplete turn");
    }
    Ok(())
}

pub(crate) async fn run_agent<M: AgentModel, T: ToolExecutor>(
    model: &mut M,
    prompt: String,
    max_iterations: Option<usize>,
    max_tool_calls: Option<usize>,
    tool_executor: &mut T,
    on_event: impl FnMut(AgentEvent),
    max_context_bytes: usize,
) -> Result<AgentRunResult> {
    let mut session = AgentSession::default();
    session.set_context_limit(max_context_bytes)?;
    session
        .run_turn(
            model,
            prompt,
            max_iterations,
            max_tool_calls,
            tool_executor,
            on_event,
        )
        .await
}

async fn run_turn_loop<M: AgentModel, T: ToolExecutor>(
    model: &mut M,
    state: &mut AgentState,
    tool_executor: &mut T,
    mut on_event: impl FnMut(AgentEvent),
    max_context_bytes: usize,
    full_history: &mut Vec<AgentMessage>,
) -> Result<AgentRunResult> {
    let mut tool_counts = BTreeMap::<String, usize>::new();
    let mut effects = Vec::<ToolEffect>::new();
    loop {
        state.begin_iteration()?;

        check_context_size(
            &state.messages,
            max_context_bytes,
            ContextLimitStage::TurnInProgress,
        )?;

        on_event(AgentEvent::ModelStarted {
            iteration: state.iteration,
        });
        let started = Instant::now();
        let decision = model.next_decision(&state.messages).await;
        on_event(AgentEvent::ModelFinished {
            iteration: state.iteration,
            elapsed: started.elapsed(),
            is_error: decision.is_err(),
        });
        let decision = decision?;
        match decision {
            ModelDecision::FinalAnswer(answer) => {
                state.record_message(AgentMessage::Assistant(answer.clone()), full_history);
                return Ok(AgentRunResult {
                    answer,
                    iterations: state.iteration,
                    tool_calls: tool_counts,
                    effects,
                });
            }
            ModelDecision::ToolCalls(tool_calls) => {
                if tool_calls.is_empty() {
                    bail!("model returned an empty tool call list");
                }

                state.begin_tool_batch(tool_calls.len())?;

                state.record_message(
                    AgentMessage::AssistantToolCalls(tool_calls.clone()),
                    full_history,
                );

                for tool_call in tool_calls {
                    let count = tool_counts.entry(tool_call.name.clone()).or_default();
                    *count = count
                        .checked_add(1)
                        .ok_or_else(|| invariant(anyhow::anyhow!("tool usage count overflow")))?;
                    on_event(AgentEvent::ToolStarted {
                        name: tool_call.name.clone(),
                    });
                    let started = Instant::now();
                    let tool_output = tool_executor
                        .execute(&tool_call.name, &tool_call.input)
                        .await
                        .with_context(|| format!("tool `{}` execution failed", tool_call.name));
                    on_event(AgentEvent::ToolFinished {
                        name: tool_call.name.clone(),
                        elapsed: started.elapsed(),
                        is_error: tool_output.as_ref().map_or(true, |output| output.is_error),
                    });
                    let tool_output = tool_output?;

                    // Report what happened, not whether the tool "succeeded": a
                    // failing test run still executed and must stay visible.
                    effects.extend(tool_output.effects);
                    state.record_message(
                        AgentMessage::ToolResult {
                            tool_call_id: tool_call.id,
                            name: tool_call.name,
                            output: tool_output.content,
                            is_error: tool_output.is_error,
                        },
                        full_history,
                    );
                }
            }
        }
    }
}

pub(crate) trait ToolExecutor {
    async fn execute(&mut self, name: &str, input: &str) -> Result<ToolOutput>;
}

fn check_context_size(
    messages: &[AgentMessage],
    max_context_bytes: usize,
    stage: ContextLimitStage,
) -> Result<()> {
    let bytes = serde_json::to_vec(messages)
        .map_err(|error| invariant(error.into()).context("failed to serialize agent context"))?;

    if bytes.len() > max_context_bytes {
        return Err(ContextLimitExceeded {
            actual: bytes.len(),
            limit: max_context_bytes,
            stage,
        }
        .into());
    }
    Ok(())
}

fn first_turn_end(messages: &[AgentMessage]) -> Option<usize> {
    for (index, message) in messages.iter().enumerate() {
        if matches!(message, AgentMessage::Assistant(_)) {
            return Some(index + 1);
        }
    }
    None
}

/// Input consists of completed old turns followed by the new user message.
/// Return a turn boundary without modifying the original messages.
fn context_start(messages: &[AgentMessage], max_context_bytes: usize) -> Result<usize> {
    let mut start = 0;
    loop {
        match check_context_size(
            &messages[start..],
            max_context_bytes,
            ContextLimitStage::Input,
        ) {
            std::result::Result::Ok(()) => return Ok(start),
            Err(error) if error.is::<ContextLimitExceeded>() => {
                match first_turn_end(&messages[start..]) {
                    Some(end) => start += end,
                    None => return Err(error),
                }
            }
            Err(error) => return Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::BuiltinToolExecutor;
    use std::collections::VecDeque;

    #[test]
    fn new_conversation_preserves_budget_but_clears_histories_and_closed_state() {
        let mut old = AgentSession::from_history(vec![
            AgentMessage::User("old".into()),
            AgentMessage::Assistant("answer".into()),
        ])
        .unwrap();
        old.set_context_limit(128).unwrap();
        old.closed = true;
        let new = old.new_conversation();
        assert_eq!(new.max_context_bytes, 128);
        assert!(new.full_history.is_empty());
        assert!(new.working_context.is_none());
        assert!(new.can_continue());
        assert_eq!(old.full_history.len(), 2);
    }

    #[test]
    fn context_start_keeps_history_at_exact_budget() {
        let messages = vec![
            AgentMessage::User("old question".into()),
            AgentMessage::Assistant("old answer".into()),
            AgentMessage::User("new question".into()),
        ];
        let budget = serde_json::to_vec(&messages).unwrap().len();

        assert_eq!(context_start(&messages, budget).unwrap(), 0);
    }

    #[test]
    fn context_start_removes_only_as_many_old_turns_as_needed() {
        let messages = vec![
            AgentMessage::User("first question".into()),
            AgentMessage::Assistant("first answer".into()),
            AgentMessage::User("second question".into()),
            AgentMessage::Assistant("second answer".into()),
            AgentMessage::User("new question".into()),
        ];
        let original = messages.clone();
        for expected_start in [2, 4] {
            let budget = serde_json::to_vec(&messages[expected_start..])
                .unwrap()
                .len();
            assert_eq!(context_start(&messages, budget).unwrap(), expected_start);
        }
        assert_eq!(messages, original);
    }

    #[test]
    fn context_start_removes_tool_turn_as_a_whole() {
        let messages = vec![
            AgentMessage::User("read a file".into()),
            AgentMessage::AssistantToolCalls(vec![ToolCall {
                id: "read-1".into(),
                name: "read_file".into(),
                input: r#"{"path":"src/main.rs"}"#.into(),
            }]),
            AgentMessage::ToolResult {
                tool_call_id: "read-1".into(),
                name: "read_file".into(),
                output: "file contents".into(),
                is_error: false,
            },
            AgentMessage::Assistant("done".into()),
            AgentMessage::User("next question".into()),
        ];
        let budget = serde_json::to_vec(&messages[4..]).unwrap().len();

        assert_eq!(context_start(&messages, budget).unwrap(), 4);
    }

    #[test]
    fn context_start_rejects_oversized_new_input_without_changing_history() {
        let messages = vec![
            AgentMessage::User("old question".into()),
            AgentMessage::Assistant("old answer".into()),
            AgentMessage::User("new question".repeat(100)),
        ];
        let original = messages.clone();
        let actual = serde_json::to_vec(&messages[2..]).unwrap().len();
        let error = context_start(&messages, actual - 1).unwrap_err();
        let error = error.downcast_ref::<ContextLimitExceeded>().unwrap();

        assert_eq!(error.actual, actual);
        assert_eq!(error.limit, actual - 1);
        assert_eq!(messages, original);
    }

    #[test]
    fn first_turn_end_returns_none_for_empty_history() {
        assert_eq!(first_turn_end(&[]), None);
    }

    #[test]
    fn first_turn_end_returns_first_completed_turn_boundary() {
        let messages = vec![
            AgentMessage::User("first question".into()),
            AgentMessage::Assistant("first answer".into()),
            AgentMessage::User("second question".into()),
            AgentMessage::Assistant("second answer".into()),
        ];

        assert_eq!(first_turn_end(&messages), Some(2));
    }

    #[test]
    fn first_turn_end_keeps_tool_calls_and_results_in_same_turn() {
        let messages = vec![
            AgentMessage::User("read the file".into()),
            AgentMessage::AssistantToolCalls(vec![ToolCall {
                id: "read-1".into(),
                name: "read_file".into(),
                input: r#"{"path":"src/main.rs"}"#.into(),
            }]),
            AgentMessage::ToolResult {
                tool_call_id: "read-1".into(),
                name: "read_file".into(),
                output: "fn main() {}".into(),
                is_error: false,
            },
            AgentMessage::Assistant("the file contains an empty main function".into()),
        ];

        assert_eq!(first_turn_end(&messages), Some(4));
    }

    #[tokio::test]
    async fn full_history_and_working_context_record_each_new_message_once() {
        let call = |id: &str| ToolCall {
            id: id.into(),
            name: "add".into(),
            input: "{}".into(),
        };
        let mut model = FakeModel::new(vec![
            ModelDecision::ToolCalls(vec![call("a"), call("b")]),
            ModelDecision::FinalAnswer("first answer".into()),
            ModelDecision::FinalAnswer("second answer".into()),
        ]);
        let mut tools = FakeToolExecutor {
            output: ToolOutput::success("result".into()),
            calls: vec![],
        };
        let mut session = AgentSession::default();
        session
            .run_turn(&mut model, "first".into(), None, None, &mut tools, |_| {})
            .await
            .unwrap();
        session
            .run_turn(&mut model, "second".into(), None, None, &mut tools, |_| {})
            .await
            .unwrap();
        let history = session.completed_messages().unwrap();
        assert_eq!(history.len(), 7);
        assert_eq!(session.working_context.as_deref().unwrap(), history);
        assert_eq!(model.received_messages[2], history[..6]);
        validate_history(history).unwrap();
    }

    #[tokio::test]
    async fn budget_and_model_use_working_context_without_losing_original_history() {
        let original = vec![
            AgentMessage::User("old input".repeat(256)),
            AgentMessage::Assistant("old answer".into()),
            AgentMessage::User("recent input".into()),
            AgentMessage::Assistant("recent answer".into()),
        ];
        let mut session = AgentSession::from_history(original.clone()).unwrap();
        assert_eq!(session.working_context.as_deref().unwrap(), original);
        // Preflight must remove the oversized oldest turn automatically.
        session.set_context_limit(512).unwrap();
        assert!(check_context_size(&original, 512, ContextLimitStage::Input).is_err());
        let mut model = FakeModel::new(vec![ModelDecision::FinalAnswer("continued".into())]);
        let mut tools = FakeToolExecutor {
            output: ToolOutput::success("unused".into()),
            calls: vec![],
        };
        session
            .run_turn(&mut model, "next".into(), None, None, &mut tools, |_| {})
            .await
            .unwrap();
        assert_eq!(
            model.received_messages[0],
            vec![
                original[2].clone(),
                original[3].clone(),
                AgentMessage::User("next".into())
            ]
        );
        let history = session.completed_messages().unwrap();
        assert_eq!(&history[..4], original);
        assert_eq!(history.len(), 6);
        assert_eq!(session.working_context.as_ref().unwrap().len(), 4);
        validate_history(history).unwrap();
    }

    #[tokio::test]
    async fn rejected_input_leaves_both_histories_unchanged_after_restore() {
        let original = vec![
            AgentMessage::User("first".into()),
            AgentMessage::Assistant("answer".into()),
        ];
        let mut session = AgentSession::from_history(original.clone()).unwrap();
        session.set_context_limit(256).unwrap();
        let mut model = FakeModel::new(vec![]);
        let mut tools = FakeToolExecutor {
            output: ToolOutput::success("unused".into()),
            calls: vec![],
        };
        assert!(
            session
                .run_turn(&mut model, "x".repeat(1024), None, None, &mut tools, |_| {})
                .await
                .unwrap_err()
                .is::<ContextLimitExceeded>()
        );
        assert_eq!(session.full_history, original);
        assert_eq!(session.working_context.as_deref().unwrap(), original);
        assert!(session.can_continue());
        assert!(model.received_messages.is_empty());
    }

    #[test]
    fn context_budget_counts_utf8_json_and_accepts_exact_boundary() {
        let messages = vec![AgentMessage::User("你好\n\"Rust\"".into())];
        let size = serde_json::to_vec(&messages).unwrap().len();
        assert!(size > "你好\n\"Rust\"".chars().count());
        check_context_size(&messages, size, ContextLimitStage::Input).unwrap();
        let error = check_context_size(&messages, size - 1, ContextLimitStage::Input).unwrap_err();
        let error = error.downcast_ref::<ContextLimitExceeded>().unwrap();
        assert_eq!(error.actual, size);
        assert_eq!(error.limit, size - 1);
        assert!(AgentSession::default().set_context_limit(0).is_err());
    }

    #[tokio::test]
    async fn oversized_input_preserves_history_and_allows_retry_without_model_call() {
        let mut session = AgentSession::default();
        session.set_context_limit(512).unwrap();
        let mut model = FakeModel::new(vec![
            ModelDecision::FinalAnswer("hi".into()),
            ModelDecision::FinalAnswer("continued".into()),
        ]);
        let mut tools = FakeToolExecutor {
            output: ToolOutput::success("unused".into()),
            calls: vec![],
        };
        session
            .run_turn(&mut model, "hello".into(), None, None, &mut tools, |_| {})
            .await
            .unwrap();
        let before = session.completed_messages().unwrap().to_vec();
        let mut events = vec![];
        let error = session
            .run_turn(
                &mut model,
                "x".repeat(1024),
                None,
                None,
                &mut tools,
                |event| events.push(event),
            )
            .await
            .unwrap_err();
        assert!(error.is::<ContextLimitExceeded>());
        assert!(session.can_continue());
        assert_eq!(session.completed_messages().unwrap(), before);
        assert_eq!(model.received_messages.len(), 1);
        assert!(events.is_empty());
        assert!(tools.calls.is_empty());
        session
            .run_turn(&mut model, "ok".into(), None, None, &mut tools, |_| {})
            .await
            .unwrap();
        assert_eq!(model.received_messages[1].len(), 3);
        assert_eq!(
            model.received_messages[1][2],
            AgentMessage::User("ok".into())
        );
    }

    #[tokio::test]
    async fn tool_output_growth_rolls_back_the_turn_and_keeps_the_session_usable() {
        let mut session = AgentSession::default();
        session.set_context_limit(512).unwrap();
        let mut model = FakeModel::new(vec![
            ModelDecision::ToolCalls(vec![ToolCall {
                id: "call".into(),
                name: "read_file".into(),
                input: "{}".into(),
            }]),
            ModelDecision::FinalAnswer("after rollback".into()),
        ]);
        let mut tools = FakeToolExecutor {
            output: ToolOutput::success("x".repeat(1024)),
            calls: vec![],
        };
        let error = session
            .run_turn(&mut model, "hello".into(), None, None, &mut tools, |_| {})
            .await
            .unwrap_err();
        let limit = error.downcast_ref::<ContextLimitExceeded>().unwrap();
        assert_eq!(limit.stage, ContextLimitStage::TurnInProgress);
        assert!(session.can_continue());
        assert_eq!(session.full_history, Vec::<AgentMessage>::new());
        assert!(session.working_context.is_none());
        assert!(session.completed_messages().is_err());

        // The failed turn is gone: the next request contains only the new input.
        let result = session
            .run_turn(&mut model, "again".into(), None, None, &mut tools, |_| {})
            .await
            .unwrap();
        assert_eq!(result.answer, "after rollback");
        assert_eq!(
            model.received_messages[1],
            vec![AgentMessage::User("again".into())]
        );
        assert_eq!(
            session.completed_messages().unwrap(),
            [
                AgentMessage::User("again".into()),
                AgentMessage::Assistant("after rollback".into())
            ]
        );
    }

    #[tokio::test]
    async fn model_failure_after_tool_results_rolls_back_and_hides_the_tool_messages() {
        let mut session = AgentSession::default();
        let mut model = ScriptedModel::new(vec![
            ScriptedStep::Decide(ModelDecision::FinalAnswer("first".into())),
            ScriptedStep::Decide(ModelDecision::ToolCalls(vec![ToolCall {
                id: "call".into(),
                name: "read_file".into(),
                input: "{}".into(),
            }])),
            ScriptedStep::Fail,
            ScriptedStep::Decide(ModelDecision::FinalAnswer("recovered".into())),
        ]);
        let mut tools = FakeToolExecutor {
            output: ToolOutput::success("tool output".into()),
            calls: vec![],
        };
        session
            .run_turn(&mut model, "one".into(), None, None, &mut tools, |_| {})
            .await
            .unwrap();
        let completed = session.completed_messages().unwrap().to_vec();
        let error = session
            .run_turn(&mut model, "two".into(), None, None, &mut tools, |_| {})
            .await
            .unwrap_err();
        assert!(error.to_string().contains("model unavailable"));
        assert!(session.can_continue());
        assert_eq!(session.completed_messages().unwrap(), completed);

        let result = session
            .run_turn(&mut model, "three".into(), None, None, &mut tools, |_| {})
            .await
            .unwrap();
        assert_eq!(result.answer, "recovered");
        let mut expected = completed;
        expected.extend([
            AgentMessage::User("three".into()),
            AgentMessage::Assistant("recovered".into()),
        ]);
        assert_eq!(session.completed_messages().unwrap(), expected);
        assert_eq!(
            model.received_messages[3],
            [
                AgentMessage::User("one".into()),
                AgentMessage::Assistant("first".into()),
                AgentMessage::User("three".into()),
            ]
        );
    }

    #[tokio::test]
    async fn fatal_tool_error_rolls_back_and_allows_a_retry() {
        let mut session = AgentSession::default();
        let mut model = FakeModel::new(vec![
            ModelDecision::ToolCalls(vec![ToolCall {
                id: "call".into(),
                name: "add".into(),
                input: "{}".into(),
            }]),
            ModelDecision::FinalAnswer("retried".into()),
        ]);
        let mut tools = FailingToolExecutor { calls: Vec::new() };
        let error = session
            .run_turn(&mut model, "first".into(), None, None, &mut tools, |_| {})
            .await
            .unwrap_err();
        assert!(error.to_string().contains("execution failed"));
        assert!(session.can_continue());
        assert_eq!(session.full_history, Vec::<AgentMessage>::new());

        let result = session
            .run_turn(&mut model, "second".into(), None, None, &mut tools, |_| {})
            .await
            .unwrap();
        assert_eq!(result.answer, "retried");
        assert_eq!(
            model.received_messages[1],
            vec![AgentMessage::User("second".into())]
        );
    }

    #[test]
    fn invariant_failures_close_the_session() {
        let mut state = AgentState::new("hello".into(), None, None).unwrap();
        state.iteration = usize::MAX;
        let error = state.begin_iteration().unwrap_err();
        assert!(error.is::<SessionInvariant>());
        assert!(error.to_string().contains("iteration count overflow"));

        let mut session = AgentSession {
            pending_turn: Some(TurnCheckpoint {
                full_history_len: 0,
                working_context: None,
            }),
            closed: true,
            ..AgentSession::default()
        };
        let error = session.fail_turn(invariant(anyhow::anyhow!("broken invariant")));
        assert!(error.is::<SessionInvariant>());
        assert!(!session.can_continue());
        assert!(session.completed_messages().is_err());
    }

    #[test]
    fn abort_turn_restores_a_dropped_turn_and_is_idempotent() {
        let original = vec![
            AgentMessage::User("first".into()),
            AgentMessage::Assistant("answer".into()),
        ];
        let mut session = AgentSession::from_history(original.clone()).unwrap();
        session.pending_turn = Some(TurnCheckpoint {
            full_history_len: original.len(),
            working_context: Some(original.clone()),
        });
        session
            .full_history
            .push(AgentMessage::User("partial".into()));
        session.working_context = None;
        session.closed = true;

        session.abort_turn();
        assert_eq!(session.full_history, original);
        assert_eq!(
            session.working_context.as_deref(),
            Some(original.as_slice())
        );
        assert!(session.can_continue());

        session.abort_turn();
        assert_eq!(session.full_history, original);
        assert_eq!(
            session.working_context.as_deref(),
            Some(original.as_slice())
        );
    }

    struct FakeToolExecutor {
        output: ToolOutput,
        calls: Vec<(String, String)>,
    }

    impl ToolExecutor for FakeToolExecutor {
        async fn execute(&mut self, name: &str, input: &str) -> Result<ToolOutput> {
            self.calls.push((name.to_string(), input.to_string()));
            Ok(self.output.clone())
        }
    }

    enum ScriptedStep {
        Decide(ModelDecision),
        Fail,
    }

    struct ScriptedModel {
        steps: VecDeque<ScriptedStep>,
        received_messages: Vec<Vec<AgentMessage>>,
    }

    impl ScriptedModel {
        fn new(steps: Vec<ScriptedStep>) -> Self {
            Self {
                steps: steps.into(),
                received_messages: Vec::new(),
            }
        }
    }

    impl AgentModel for ScriptedModel {
        async fn next_decision(&mut self, messages: &[AgentMessage]) -> Result<ModelDecision> {
            self.received_messages.push(messages.to_vec());
            match self.steps.pop_front() {
                Some(ScriptedStep::Decide(decision)) => Ok(decision),
                Some(ScriptedStep::Fail) | None => bail!("model unavailable"),
            }
        }
    }

    struct FakeModel {
        decisions: VecDeque<ModelDecision>,
        received_messages: Vec<Vec<AgentMessage>>,
    }

    impl FakeModel {
        fn new(decisions: Vec<ModelDecision>) -> Self {
            Self {
                decisions: decisions.into(),
                received_messages: Vec::new(),
            }
        }
    }

    impl AgentModel for FakeModel {
        async fn next_decision(&mut self, messages: &[AgentMessage]) -> Result<ModelDecision> {
            self.received_messages.push(messages.to_vec());
            self.decisions
                .pop_front()
                .ok_or_else(|| anyhow::anyhow!("no more decisions available"))
        }
    }

    #[test]
    fn creates_initial_agent_state() {
        let state = AgentState::new("hello".to_string(), Some(3), Some(32)).unwrap();
        assert_eq!(
            state.messages,
            vec![AgentMessage::User("hello".to_string())]
        );
        assert_eq!(state.iteration, 0);
        assert_eq!(state.max_iterations, Some(3));
    }

    #[test]
    fn rejects_blank_prompt() {
        let result = AgentState::new("   \n\t".to_string(), Some(3), Some(32));
        let message = result.unwrap_err().to_string();
        assert_eq!(message, "agent prompt cannot be empty");
    }

    #[test]
    fn rejects_zero_max_tool_calls() {
        let error = AgentState::new("hello".to_string(), Some(3), Some(0)).unwrap_err();
        assert_eq!(error.to_string(), "max_tool_calls must be greater than 0");
    }

    #[tokio::test]
    async fn runs_without_budgets_past_previous_defaults() {
        let call = ToolCall {
            id: "call-1".to_string(),
            name: "add".to_string(),
            input: "2,3".to_string(),
        };
        let mut decisions = Vec::new();
        for _ in 0..10 {
            decisions.push(ModelDecision::ToolCalls(vec![call.clone(); 4]));
        }
        decisions.push(ModelDecision::FinalAnswer("done".to_string()));
        let mut model = FakeModel::new(decisions);
        let mut tool = FakeToolExecutor {
            output: ToolOutput::success("5".to_string()),
            calls: Vec::new(),
        };
        let answer = run_agent(
            &mut model,
            "hello".to_string(),
            None,
            None,
            &mut tool,
            |_| {},
            DEFAULT_MAX_CONTEXT_BYTES,
        )
        .await
        .unwrap();
        assert_eq!(answer.answer, "done");
        assert_eq!(tool.calls.len(), 40);
        assert_eq!(model.received_messages.len(), 11);
    }

    #[test]
    fn budgets_can_be_configured_independently() {
        let mut turns_only = AgentState::new("hello".to_string(), Some(1), None).unwrap();
        turns_only.begin_tool_batch(100).unwrap();
        turns_only.begin_iteration().unwrap();
        assert!(turns_only.begin_iteration().is_err());

        let mut tools_only = AgentState::new("hello".to_string(), None, Some(1)).unwrap();
        for _ in 0..20 {
            tools_only.begin_iteration().unwrap();
        }
        tools_only.begin_tool_batch(1).unwrap();
        assert!(tools_only.begin_tool_batch(1).is_err());
    }

    #[test]
    fn tool_budget_allows_exact_limit_and_preserves_count_on_failure() {
        let mut state = AgentState::new("hello".to_string(), Some(8), Some(3)).unwrap();
        state.begin_tool_batch(2).unwrap();
        state.begin_tool_batch(1).unwrap();
        assert_eq!(state.tool_calls, 3);
        let error = state.begin_tool_batch(1).unwrap_err();
        assert_eq!(error.to_string(), "agent reached maximum tool calls");
        assert_eq!(state.tool_calls, 3);
    }

    #[tokio::test]
    async fn rejects_over_budget_batches_without_partial_execution() {
        // Exercise both an oversized first batch and a later cumulative overrun.
        for first_batch in [0, 1] {
            let call = ToolCall {
                id: "call-1".to_string(),
                name: "add".to_string(),
                input: "2,3".to_string(),
            };
            let mut decisions = Vec::new();
            if first_batch == 1 {
                decisions.push(ModelDecision::ToolCalls(vec![call.clone()]));
            }
            decisions.push(ModelDecision::ToolCalls(vec![call.clone(), call]));
            let mut model = FakeModel::new(decisions);
            let mut tool = FakeToolExecutor {
                output: ToolOutput::success("5".to_string()),
                calls: Vec::new(),
            };
            let error = run_agent(
                &mut model,
                "hello".to_string(),
                Some(8),
                Some(1),
                &mut tool,
                |_| {},
                DEFAULT_MAX_CONTEXT_BYTES,
            )
            .await
            .unwrap_err();
            assert_eq!(error.to_string(), "agent reached maximum tool calls");
            assert_eq!(tool.calls.len(), first_batch);
            assert_eq!(model.received_messages.len(), first_batch + 1);
        }
    }

    #[tokio::test]
    async fn allows_final_answer_after_tool_budget_is_exhausted() {
        let mut model = FakeModel::new(vec![
            ModelDecision::ToolCalls(vec![ToolCall {
                id: "call-1".to_string(),
                name: "add".to_string(),
                input: "2,3".to_string(),
            }]),
            ModelDecision::FinalAnswer("5".to_string()),
        ]);
        let mut tool = FakeToolExecutor {
            output: ToolOutput::success("5".to_string()),
            calls: Vec::new(),
        };
        let answer = run_agent(
            &mut model,
            "hello".to_string(),
            Some(2),
            Some(1),
            &mut tool,
            |_| {},
            DEFAULT_MAX_CONTEXT_BYTES,
        )
        .await
        .unwrap();
        assert_eq!(answer.answer, "5");
        assert_eq!(tool.calls.len(), 1);
    }

    #[test]
    fn rejects_zero_max_iterations() {
        let result = AgentState::new("hello".to_string(), Some(0), Some(32));
        let message = result.unwrap_err().to_string();
        assert_eq!(message, "max_iterations must be greater than 0");
    }

    #[test]
    fn allows_calls_up_to_max_iterations() {
        let mut state = AgentState::new("hello".to_string(), Some(3), Some(32)).unwrap();
        for _ in 0..3 {
            state.begin_iteration().unwrap();
        }
        assert_eq!(state.iteration, 3);
        let err = state.begin_iteration().unwrap_err();
        assert_eq!(err.to_string(), "agent reached maximum iterations");
        assert_eq!(state.iteration, 3);
    }

    #[tokio::test]
    async fn fake_model_returns_decisions_in_order() {
        let mut model = FakeModel::new(vec![
            ModelDecision::ToolCalls(vec![ToolCall {
                id: "call-1".to_string(),
                name: "add".to_string(),
                input: "2,3".to_string(),
            }]),
            ModelDecision::FinalAnswer("5".to_string()),
        ]);

        let messages = vec![AgentMessage::User("add 2 and 3".to_string())];

        let first = model.next_decision(&messages).await.unwrap();

        assert_eq!(
            first,
            ModelDecision::ToolCalls(vec![ToolCall {
                id: "call-1".to_string(),
                name: "add".to_string(),
                input: "2,3".to_string(),
            }])
        );

        let answer = model.next_decision(&messages).await.unwrap();
        assert_eq!(answer, ModelDecision::FinalAnswer("5".to_string()));

        let err = model.next_decision(&messages).await.unwrap_err();
        assert_eq!(err.to_string(), "no more decisions available");
    }

    #[tokio::test]
    async fn returns_final_answer_from_model() {
        let mut model = FakeModel::new(vec![ModelDecision::FinalAnswer("done".to_string())]);

        let mut tool = FakeToolExecutor {
            output: ToolOutput::success("5".to_string()),
            calls: Vec::new(),
        };

        let answer = run_agent(
            &mut model,
            "hello".to_string(),
            Some(3),
            Some(32),
            &mut tool,
            |_| {},
            DEFAULT_MAX_CONTEXT_BYTES,
        )
        .await
        .unwrap();
        assert_eq!(answer.answer, "done");
        assert_eq!(answer.iterations, 1);
        assert!(answer.tool_calls.is_empty());
        assert!(tool.calls.is_empty());
    }

    #[tokio::test]
    async fn events_preserve_order_and_report_recoverable_tool_failure() {
        let mut model = FakeModel::new(vec![
            ModelDecision::ToolCalls(vec![ToolCall {
                id: "1".into(),
                name: "add".into(),
                input: "private input".into(),
            }]),
            ModelDecision::FinalAnswer("done".into()),
        ]);
        for is_error in [false, true] {
            // Replay the same decision sequence without depending on real time or HTTP.
            model.decisions = vec![
                ModelDecision::ToolCalls(vec![ToolCall {
                    id: "1".into(),
                    name: "add".into(),
                    input: "private input".into(),
                }]),
                ModelDecision::FinalAnswer("done".into()),
            ]
            .into();
            let mut tools = FakeToolExecutor {
                output: ToolOutput {
                    content: "private output".into(),
                    is_error,
                    effects: Vec::new(),
                },
                calls: Vec::new(),
            };
            let mut events = Vec::new();
            run_agent(
                &mut model,
                "hello".into(),
                None,
                None,
                &mut tools,
                |event| events.push(event),
                DEFAULT_MAX_CONTEXT_BYTES,
            )
            .await
            .unwrap();
            assert!(matches!(events.as_slice(), [
                AgentEvent::ModelStarted { iteration: 1 },
                AgentEvent::ModelFinished { iteration: 1, is_error: false, .. },
                AgentEvent::ToolStarted { name },
                AgentEvent::ToolFinished { is_error: failed, .. },
                AgentEvent::ModelStarted { iteration: 2 },
                AgentEvent::ModelFinished { iteration: 2, is_error: false, .. },
            ] if name == "add" && *failed == is_error));
            let logged = format!("{events:?}");
            assert!(!logged.contains("private input"));
            assert!(!logged.contains("private output"));
        }
    }

    #[tokio::test]
    async fn events_report_model_failure_before_propagating_error() {
        let mut model = FakeModel::new(vec![]);
        let mut tools = FakeToolExecutor {
            output: ToolOutput::success("unused".into()),
            calls: vec![],
        };
        let mut events = Vec::new();
        assert!(
            run_agent(
                &mut model,
                "hello".into(),
                None,
                None,
                &mut tools,
                |event| events.push(event),
                DEFAULT_MAX_CONTEXT_BYTES
            )
            .await
            .is_err()
        );
        assert!(matches!(
            events.as_slice(),
            [
                AgentEvent::ModelStarted { iteration: 1 },
                AgentEvent::ModelFinished {
                    iteration: 1,
                    is_error: true,
                    ..
                },
            ]
        ));
    }

    #[tokio::test]
    async fn events_report_fatal_tool_failure_before_propagating_error() {
        let mut model = FakeModel::new(vec![ModelDecision::ToolCalls(vec![ToolCall {
            id: "1".into(),
            name: "add".into(),
            input: "private".into(),
        }])]);
        let mut tools = FailingToolExecutor { calls: vec![] };
        let mut events = Vec::new();
        let error = run_agent(
            &mut model,
            "hello".into(),
            None,
            None,
            &mut tools,
            |event| events.push(event),
            DEFAULT_MAX_CONTEXT_BYTES,
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("tool `add` execution failed"));
        assert!(matches!(
            events.as_slice(),
            [
                AgentEvent::ModelStarted { iteration: 1 },
                AgentEvent::ModelFinished {
                    iteration: 1,
                    is_error: false,
                    ..
                },
                AgentEvent::ToolStarted { .. },
                AgentEvent::ToolFinished { is_error: true, .. },
            ]
        ));
    }

    #[tokio::test]
    async fn session_preserves_answers_and_tools_but_resets_turn_budgets() {
        let mut model = FakeModel::new(vec![
            ModelDecision::ToolCalls(vec![ToolCall {
                id: "call-1".into(),
                name: "add".into(),
                input: "2,3".into(),
            }]),
            ModelDecision::FinalAnswer("1. clear the file".into()),
            ModelDecision::FinalAnswer("confirmed".into()),
        ]);
        let mut tool = FakeToolExecutor {
            output: ToolOutput::success("5".into()),
            calls: Vec::new(),
        };
        let mut session = AgentSession::default();
        let first = session
            .run_turn(
                &mut model,
                "options?".into(),
                Some(2),
                Some(1),
                &mut tool,
                |_| {},
            )
            .await
            .unwrap();
        let second = session
            .run_turn(&mut model, "1".into(), Some(1), Some(1), &mut tool, |_| {})
            .await
            .unwrap();
        assert_eq!(first.iterations, 2);
        assert_eq!(first.tool_calls["add"], 1);
        assert_eq!(second.iterations, 1);
        assert!(second.tool_calls.is_empty());
        assert_eq!(
            model.received_messages[2],
            vec![
                AgentMessage::User("options?".into()),
                AgentMessage::AssistantToolCalls(vec![ToolCall {
                    id: "call-1".into(),
                    name: "add".into(),
                    input: "2,3".into(),
                }]),
                AgentMessage::ToolResult {
                    tool_call_id: "call-1".into(),
                    name: "add".into(),
                    output: "5".into(),
                    is_error: false,
                },
                AgentMessage::Assistant("1. clear the file".into()),
                AgentMessage::User("1".into()),
            ]
        );
    }

    #[tokio::test]
    async fn empty_tool_call_list_rolls_back_and_allows_retry() {
        let mut session = AgentSession::default();
        assert!(session.completed_messages().is_err());
        let mut model = FakeModel::new(vec![
            ModelDecision::ToolCalls(vec![]),
            ModelDecision::FinalAnswer("recovered".into()),
        ]);
        let mut tool = FakeToolExecutor {
            output: ToolOutput::success("unused".into()),
            calls: Vec::new(),
        };
        assert!(
            session
                .run_turn(&mut model, "hello".into(), None, None, &mut tool, |_| {})
                .await
                .is_err()
        );
        assert!(session.can_continue());
        assert_eq!(session.full_history, Vec::<AgentMessage>::new());

        let result = session
            .run_turn(&mut model, "retry".into(), None, None, &mut tool, |_| {})
            .await
            .unwrap();
        assert_eq!(result.answer, "recovered");
        assert_eq!(
            model.received_messages[1],
            vec![AgentMessage::User("retry".into())]
        );
        assert_eq!(session.completed_messages().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn counts_tools_across_rounds_including_recoverable_failures() {
        let call = |id: &str, name: &str| ToolCall {
            id: id.to_string(),
            name: name.to_string(),
            input: r#"{"a":2,"b":3}"#.to_string(),
        };
        let mut model = FakeModel::new(vec![
            ModelDecision::ToolCalls(vec![call("1", "add"), call("2", "unknown")]),
            ModelDecision::ToolCalls(vec![call("3", "add")]),
            ModelDecision::FinalAnswer("done".to_string()),
        ]);
        let directory = tempfile::tempdir().unwrap();
        let workspace = crate::workspace::Workspace::new(directory.path()).unwrap();
        let mut tools = BuiltinToolExecutor::new(workspace);

        let result = run_agent(
            &mut model,
            "hello".to_string(),
            None,
            None,
            &mut tools,
            |_| {},
            DEFAULT_MAX_CONTEXT_BYTES,
        )
        .await
        .unwrap();

        assert_eq!(result.answer, "done");
        assert_eq!(result.iterations, 3);
        assert_eq!(
            result.tool_calls,
            BTreeMap::from([("add".to_string(), 2), ("unknown".to_string(), 1),])
        );
    }

    #[tokio::test]
    async fn turn_effects_are_ordered_and_never_sent_to_the_model() {
        let effect = || ToolEffect::ModifiedFile {
            path: "a.rs".into(),
            edits: 1,
        };
        let call = |id: &str| ToolCall {
            id: id.to_string(),
            name: "replace_text".to_string(),
            input: r#"{"path":"a.rs","old_text":"a","new_text":"b"}"#.to_string(),
        };
        let mut model = FakeModel::new(vec![
            ModelDecision::ToolCalls(vec![call("1")]),
            ModelDecision::ToolCalls(vec![call("2")]),
            ModelDecision::FinalAnswer("done".to_string()),
        ]);
        let mut tool = FakeToolExecutor {
            output: ToolOutput::success("ok".to_string()).with_effects(vec![effect()]),
            calls: Vec::new(),
        };

        let result = run_agent(
            &mut model,
            "go".to_string(),
            None,
            None,
            &mut tool,
            |_| {},
            DEFAULT_MAX_CONTEXT_BYTES,
        )
        .await
        .unwrap();

        // Duplicates and order are preserved; aggregation is a display concern.
        assert_eq!(result.effects, vec![effect(), effect()]);
        // Tool results sent to the model carry content only, not effects.
        assert_eq!(
            model.received_messages[1][2],
            AgentMessage::ToolResult {
                tool_call_id: "1".into(),
                name: "replace_text".into(),
                output: "ok".into(),
                is_error: false,
            }
        );
    }

    #[tokio::test]
    async fn fake_tool_executor_records_call_and_returns_output() {
        let mut tool = FakeToolExecutor {
            output: ToolOutput::success("5".to_string()),
            calls: Vec::new(),
        };
        let result = tool.execute("add", "2,3").await.unwrap();

        assert_eq!(result, ToolOutput::success("5".to_string()));
        assert_eq!(tool.calls, vec![("add".to_string(), "2,3".to_string())]);
    }

    #[tokio::test]
    async fn agent_executes_tool_then_returns_final_answer() {
        let mut model = FakeModel::new(vec![
            ModelDecision::ToolCalls(vec![ToolCall {
                id: "call-1".to_string(),
                name: "add".to_string(),
                input: "2,3".to_string(),
            }]),
            ModelDecision::FinalAnswer("5".to_string()),
        ]);
        let mut tool = FakeToolExecutor {
            output: ToolOutput::success("5".to_string()),
            calls: Vec::new(),
        };

        let answer = run_agent(
            &mut model,
            "hello".to_string(),
            Some(2),
            Some(32),
            &mut tool,
            |_| {},
            DEFAULT_MAX_CONTEXT_BYTES,
        )
        .await
        .unwrap();
        assert_eq!(answer.answer, "5");
        assert_eq!(tool.calls, vec![("add".to_string(), "2,3".to_string())]);

        assert_eq!(
            model.received_messages,
            vec![
                vec![AgentMessage::User("hello".to_string())],
                vec![
                    AgentMessage::User("hello".to_string()),
                    AgentMessage::AssistantToolCalls(vec![ToolCall {
                        id: "call-1".to_string(),
                        name: "add".to_string(),
                        input: "2,3".to_string(),
                    }]),
                    AgentMessage::ToolResult {
                        tool_call_id: "call-1".to_string(),
                        name: "add".to_string(),
                        output: "5".to_string(),
                        is_error: false,
                    }
                ],
            ]
        );
    }

    #[tokio::test]
    async fn agent_executes_multiple_tool_calls_from_one_decision() {
        let mut model = FakeModel::new(vec![
            ModelDecision::ToolCalls(vec![
                ToolCall {
                    id: "call-1".to_string(),
                    name: "read_file".to_string(),
                    input: r#"{"path":"a.txt"}"#.to_string(),
                },
                ToolCall {
                    id: "call-2".to_string(),
                    name: "read_file".to_string(),
                    input: r#"{"path":"b.txt"}"#.to_string(),
                },
            ]),
            ModelDecision::FinalAnswer("read both".to_string()),
        ]);
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("a.txt"), "first\n").unwrap();
        std::fs::write(directory.path().join("b.txt"), "second\n").unwrap();
        let workspace = crate::workspace::Workspace::new(directory.path()).unwrap();
        let mut tool_executor = BuiltinToolExecutor::new(workspace);

        let answer = run_agent(
            &mut model,
            "read both files".to_string(),
            Some(2),
            Some(32),
            &mut tool_executor,
            |_| {},
            DEFAULT_MAX_CONTEXT_BYTES,
        )
        .await
        .unwrap();

        assert_eq!(answer.answer, "read both");
        assert_eq!(answer.iterations, 2);
        assert_eq!(
            answer.tool_calls,
            BTreeMap::from([("read_file".to_string(), 2)])
        );

        let messages = &model.received_messages[1];
        assert_eq!(messages.len(), 4);
        for (index, expected) in [(2, "first\n"), (3, "second\n")] {
            let AgentMessage::ToolResult {
                output, is_error, ..
            } = &messages[index]
            else {
                panic!("expected a tool result at {index}");
            };
            assert!(!is_error);
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(output).unwrap()["content"],
                expected
            );
        }
    }

    #[tokio::test]
    async fn agent_handles_mixed_success_and_recoverable_tool_errors() {
        let mut model = FakeModel::new(vec![
            ModelDecision::ToolCalls(vec![
                ToolCall {
                    id: "call-1".to_string(),
                    name: "read_file".to_string(),
                    input: r#"{"path":"a.txt"}"#.to_string(),
                },
                ToolCall {
                    id: "call-2".to_string(),
                    name: "read_file".to_string(),
                    input: r#"{"path":""}"#.to_string(),
                },
            ]),
            ModelDecision::FinalAnswer("one call failed".to_string()),
        ]);
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("a.txt"), "first\n").unwrap();
        let workspace = crate::workspace::Workspace::new(directory.path()).unwrap();
        let mut tool_executor = BuiltinToolExecutor::new(workspace);

        let answer = run_agent(
            &mut model,
            "run two calls".to_string(),
            Some(2),
            Some(32),
            &mut tool_executor,
            |_| {},
            DEFAULT_MAX_CONTEXT_BYTES,
        )
        .await
        .unwrap();

        assert_eq!(answer.answer, "one call failed");
        let messages = &model.received_messages[1];

        let AgentMessage::ToolResult {
            tool_call_id,
            output,
            is_error,
            ..
        } = &messages[2]
        else {
            panic!("expected a successful tool result");
        };
        assert_eq!(tool_call_id, "call-1");
        assert!(!is_error);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(output).unwrap()["content"],
            "first\n"
        );

        let AgentMessage::ToolResult {
            tool_call_id,
            output,
            is_error,
            ..
        } = &messages[3]
        else {
            panic!("expected a failed tool result");
        };
        assert_eq!(tool_call_id, "call-2");
        assert!(is_error);
        assert!(output.contains("must not be empty"), "{output}");
    }

    #[tokio::test]
    async fn rejects_empty_tool_call_list() {
        let mut model = FakeModel::new(vec![ModelDecision::ToolCalls(Vec::new())]);
        let directory = tempfile::tempdir().unwrap();
        let workspace = crate::workspace::Workspace::new(directory.path()).unwrap();
        let mut tool_executor = BuiltinToolExecutor::new(workspace);

        let error = run_agent(
            &mut model,
            "hello".to_string(),
            Some(2),
            Some(32),
            &mut tool_executor,
            |_| {},
            DEFAULT_MAX_CONTEXT_BYTES,
        )
        .await
        .unwrap_err();

        assert_eq!(error.to_string(), "model returned an empty tool call list");
    }

    #[tokio::test]
    async fn stops_after_reaching_max_iterations() {
        let mut model = FakeModel::new(vec![ModelDecision::ToolCalls(vec![ToolCall {
            id: "call-1".to_string(),
            name: "add".to_string(),
            input: "2,3".to_string(),
        }])]);

        let mut tool_executor = FakeToolExecutor {
            output: ToolOutput::success("5".to_string()),
            calls: Vec::new(),
        };

        let result = run_agent(
            &mut model,
            "hello".to_string(),
            Some(1),
            Some(32),
            &mut tool_executor,
            |_| {},
            DEFAULT_MAX_CONTEXT_BYTES,
        )
        .await;

        let err = result.unwrap_err();
        assert_eq!(err.to_string(), "agent reached maximum iterations");

        assert_eq!(
            tool_executor.calls,
            vec![("add".to_string(), "2,3".to_string())]
        );
        assert_eq!(model.received_messages.len(), 1);
    }

    struct FailingToolExecutor {
        calls: Vec<(String, String)>,
    }

    impl ToolExecutor for FailingToolExecutor {
        async fn execute(&mut self, name: &str, input: &str) -> Result<ToolOutput> {
            self.calls.push((name.to_string(), input.to_string()));

            bail!("tool execution failed");
        }
    }

    #[tokio::test]
    async fn agent_stops_on_fatal_tool_executor_error() {
        let mut model = FakeModel::new(vec![ModelDecision::ToolCalls(vec![ToolCall {
            id: "call-1".to_string(),
            name: "add".to_string(),
            input: "2,3".to_string(),
        }])]);

        let mut tool_executor = FailingToolExecutor { calls: Vec::new() };

        let result = run_agent(
            &mut model,
            "hello".to_string(),
            Some(2),
            Some(32),
            &mut tool_executor,
            |_| {},
            DEFAULT_MAX_CONTEXT_BYTES,
        );

        let error = result.await.unwrap_err();
        assert_eq!(error.to_string(), "tool `add` execution failed");

        assert_eq!(
            tool_executor.calls,
            vec![("add".to_string(), "2,3".to_string())]
        );

        assert_eq!(model.received_messages.len(), 1);

        let error_chain: Vec<String> = error.chain().map(|cause| cause.to_string()).collect();

        assert_eq!(
            error_chain,
            vec![
                "tool `add` execution failed".to_string(),
                "tool execution failed".to_string(),
            ]
        );
    }

    #[tokio::test]
    async fn agent_uses_builtin_read_only_tool() {
        let mut model = FakeModel::new(vec![
            ModelDecision::ToolCalls(vec![ToolCall {
                id: "call-1".to_string(),
                name: "read_file".to_string(),
                input: r#"{"path":"notes.txt"}"#.to_string(),
            }]),
            ModelDecision::FinalAnswer("read it".to_string()),
        ]);

        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("notes.txt"), "hello\n").unwrap();
        let workspace = crate::workspace::Workspace::new(directory.path()).unwrap();
        let mut tool_executor = BuiltinToolExecutor::new(workspace);

        let result = run_agent(
            &mut model,
            "read notes.txt".to_string(),
            Some(2),
            Some(32),
            &mut tool_executor,
            |_| {},
            DEFAULT_MAX_CONTEXT_BYTES,
        )
        .await
        .unwrap();

        assert_eq!(result.answer, "read it");
        assert_eq!(model.received_messages.len(), 2);

        let messages = &model.received_messages[1];
        assert_eq!(
            messages[0],
            AgentMessage::User("read notes.txt".to_string())
        );
        assert_eq!(
            messages[1],
            AgentMessage::AssistantToolCalls(vec![ToolCall {
                id: "call-1".to_string(),
                name: "read_file".to_string(),
                input: r#"{"path":"notes.txt"}"#.to_string(),
            }])
        );
        let AgentMessage::ToolResult {
            tool_call_id,
            name,
            output,
            is_error,
        } = &messages[2]
        else {
            panic!("expected a tool result, got {:?}", messages[2]);
        };
        assert_eq!(tool_call_id, "call-1");
        assert_eq!(name, "read_file");
        assert!(!is_error);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(output).unwrap(),
            serde_json::json!({
                "path": "notes.txt",
                "start_line": 1,
                "end_line": 1,
                "content": "hello\n",
                "truncated": false
            })
        );
    }

    #[tokio::test]
    async fn agent_returns_recoverable_tool_error_to_model() {
        let mut model = FakeModel::new(vec![
            ModelDecision::ToolCalls(vec![ToolCall {
                id: "call-1".to_string(),
                name: "read_file".to_string(),
                input: r#"{"path":""}"#.to_string(),
            }]),
            ModelDecision::FinalAnswer("The tool input was invalid".to_string()),
        ]);
        let directory = tempfile::tempdir().unwrap();
        let workspace = crate::workspace::Workspace::new(directory.path()).unwrap();
        let mut tool_executor = BuiltinToolExecutor::new(workspace);

        let answer = run_agent(
            &mut model,
            "read with invalid input".to_string(),
            Some(2),
            Some(32),
            &mut tool_executor,
            |_| {},
            DEFAULT_MAX_CONTEXT_BYTES,
        )
        .await
        .unwrap();

        assert_eq!(answer.answer, "The tool input was invalid");
        let Some(AgentMessage::ToolResult {
            tool_call_id,
            name,
            output,
            is_error,
        }) = model.received_messages[1].last()
        else {
            panic!("expected a tool result");
        };
        assert_eq!(tool_call_id, "call-1");
        assert_eq!(name, "read_file");
        assert!(*is_error);
        assert!(output.contains("must not be empty"), "{output}");
    }
}
/*这里是对本文件的补充说明，也就是个人笔记
ModelDecision：这个就是模型本轮作出的决定，决定使用answer还是接着调用工具

AgentMessage：Agent的上下文历史

AgentModel：模型行为边界

AgentState：一次 Agent 运行的状态

AgentState::new()

begin_iteration()：防止无限循环

ToolExecutor：工具执行边界

run_agent()：Agent Loop 的核心控制流


*/
