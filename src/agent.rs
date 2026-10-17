use anyhow::{Context, Result, bail};

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ModelDecision {
    FinalAnswer(String),
    ToolCalls(Vec<ToolCall>),
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub(crate) struct ToolCall {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) input: String,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub(crate) enum AgentMessage {
    User(String),
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
}

impl ToolOutput {
    pub(crate) fn success(content: String) -> Self {
        Self {
            content,
            is_error: false,
        }
    }

    pub(crate) fn error(content: String) -> Self {
        Self {
            content,
            is_error: true,
        }
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
            .context("tool call count overflow")?;

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
            .context("iteration count overflow")?;
        Ok(())
    }
}

pub(crate) async fn run_agent<M: AgentModel, T: ToolExecutor>(
    model: &mut M,
    prompt: String,
    max_iterations: Option<usize>,
    max_tool_calls: Option<usize>,
    tool_executor: &mut T,
) -> Result<String> {
    let mut state = AgentState::new(prompt, max_iterations, max_tool_calls)?;
    loop {
        state.begin_iteration()?;
        let decision = model.next_decision(&state.messages).await?;
        match decision {
            ModelDecision::FinalAnswer(answer) => return Ok(answer),
            ModelDecision::ToolCalls(tool_calls) => {
                if tool_calls.is_empty() {
                    bail!("model returned an empty tool call list");
                }

                state.begin_tool_batch(tool_calls.len())?;

                state
                    .messages
                    .push(AgentMessage::AssistantToolCalls(tool_calls.clone()));

                for tool_call in tool_calls {
                    let tool_output = tool_executor
                        .execute(&tool_call.name, &tool_call.input)
                        .await
                        .with_context(|| format!("tool `{}` execution failed", tool_call.name))?;

                    state.messages.push(AgentMessage::ToolResult {
                        tool_call_id: tool_call.id,
                        name: tool_call.name,
                        output: tool_output.content,
                        is_error: tool_output.is_error,
                    });
                }
            }
        }
    }
}

pub(crate) trait ToolExecutor {
    async fn execute(&mut self, name: &str, input: &str) -> Result<ToolOutput>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::BuiltinToolExecutor;
    use std::collections::VecDeque;

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
        let answer = run_agent(&mut model, "hello".to_string(), None, None, &mut tool)
            .await
            .unwrap();
        assert_eq!(answer, "done");
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
            let error = run_agent(&mut model, "hello".to_string(), Some(8), Some(1), &mut tool)
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
        let answer = run_agent(&mut model, "hello".to_string(), Some(2), Some(1), &mut tool)
            .await
            .unwrap();
        assert_eq!(answer, "5");
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
        )
        .await
        .unwrap();
        assert_eq!(answer, "done");
        assert!(tool.calls.is_empty());
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
        )
        .await
        .unwrap();
        assert_eq!(answer, "5");
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
                    name: "add".to_string(),
                    input: r#"{"a":1,"b":2}"#.to_string(),
                },
                ToolCall {
                    id: "call-2".to_string(),
                    name: "add".to_string(),
                    input: r#"{"a":3,"b":4}"#.to_string(),
                },
            ]),
            ModelDecision::FinalAnswer("3 and 7".to_string()),
        ]);
        let directory = tempfile::tempdir().unwrap();
        let workspace = crate::workspace::Workspace::new(directory.path()).unwrap();
        let mut tool_executor = BuiltinToolExecutor::new(workspace);

        let answer = run_agent(
            &mut model,
            "give two sums".to_string(),
            Some(2),
            Some(32),
            &mut tool_executor,
        )
        .await
        .unwrap();

        assert_eq!(answer, "3 and 7");
        assert_eq!(
            model.received_messages[1],
            vec![
                AgentMessage::User("give two sums".to_string()),
                AgentMessage::AssistantToolCalls(vec![
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
                ]),
                AgentMessage::ToolResult {
                    tool_call_id: "call-1".to_string(),
                    name: "add".to_string(),
                    output: "3".to_string(),
                    is_error: false,
                },
                AgentMessage::ToolResult {
                    tool_call_id: "call-2".to_string(),
                    name: "add".to_string(),
                    output: "7".to_string(),
                    is_error: false,
                },
            ]
        );
    }

    #[tokio::test]
    async fn agent_handles_mixed_success_and_recoverable_tool_errors() {
        let mut model = FakeModel::new(vec![
            ModelDecision::ToolCalls(vec![
                ToolCall {
                    id: "call-1".to_string(),
                    name: "add".to_string(),
                    input: r#"{"a":1,"b":2}"#.to_string(),
                },
                ToolCall {
                    id: "call-2".to_string(),
                    name: "add".to_string(),
                    input: r#"{"a":3}"#.to_string(),
                },
            ]),
            ModelDecision::FinalAnswer("one call failed".to_string()),
        ]);
        let directory = tempfile::tempdir().unwrap();
        let workspace = crate::workspace::Workspace::new(directory.path()).unwrap();
        let mut tool_executor = BuiltinToolExecutor::new(workspace);

        let answer = run_agent(
            &mut model,
            "run two calls".to_string(),
            Some(2),
            Some(32),
            &mut tool_executor,
        )
        .await
        .unwrap();

        assert_eq!(answer, "one call failed");
        assert_eq!(
            &model.received_messages[1][2..],
            [
                AgentMessage::ToolResult {
                    tool_call_id: "call-1".to_string(),
                    name: "add".to_string(),
                    output: "3".to_string(),
                    is_error: false,
                },
                AgentMessage::ToolResult {
                    tool_call_id: "call-2".to_string(),
                    name: "add".to_string(),
                    output: "invalid add tool input".to_string(),
                    is_error: true,
                },
            ]
        );
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
    async fn agent_uses_builtin_add_tool() {
        let mut model = FakeModel::new(vec![
            ModelDecision::ToolCalls(vec![ToolCall {
                id: "call-1".to_string(),
                name: "add".to_string(),
                input: r#"{"a":2,"b":3}"#.to_string(),
            }]),
            ModelDecision::FinalAnswer("5".to_string()),
        ]);

        let directory = tempfile::tempdir().unwrap();
        let workspace = crate::workspace::Workspace::new(directory.path()).unwrap();
        let mut tool_executor = BuiltinToolExecutor::new(workspace);

        let result = run_agent(
            &mut model,
            "add 2 and 3".to_string(),
            Some(2),
            Some(32),
            &mut tool_executor,
        )
        .await
        .unwrap();

        assert_eq!(result, "5");
        assert_eq!(model.received_messages.len(), 2);

        assert_eq!(
            model.received_messages[1],
            vec![
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
            ]
        );
    }

    #[tokio::test]
    async fn agent_returns_recoverable_tool_error_to_model() {
        let mut model = FakeModel::new(vec![
            ModelDecision::ToolCalls(vec![ToolCall {
                id: "call-1".to_string(),
                name: "add".to_string(),
                input: r#"{"a":2}"#.to_string(),
            }]),
            ModelDecision::FinalAnswer("The tool input was invalid".to_string()),
        ]);
        let directory = tempfile::tempdir().unwrap();
        let workspace = crate::workspace::Workspace::new(directory.path()).unwrap();
        let mut tool_executor = BuiltinToolExecutor::new(workspace);

        let answer = run_agent(
            &mut model,
            "add with invalid input".to_string(),
            Some(2),
            Some(32),
            &mut tool_executor,
        )
        .await
        .unwrap();

        assert_eq!(answer, "The tool input was invalid");
        assert_eq!(
            model.received_messages[1].last(),
            Some(&AgentMessage::ToolResult {
                tool_call_id: "call-1".to_string(),
                name: "add".to_string(),
                output: "invalid add tool input".to_string(),
                is_error: true,
            })
        );
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
