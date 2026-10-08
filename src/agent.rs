use anyhow::{Context, Result, bail};

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ModelDecision {
    FinalAnswer(String),
    ToolCall {
        id: String,
        name: String,
        input: String,
    },
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub(crate) enum AgentMessage {
    User(String),
    ToolCall {
        id: String,
        name: String,
        input: String,
    },
    ToolResult {
        tool_call_id: String,
        name: String,
        output: String,
    },
}

pub(crate) trait AgentModel {
    async fn next_decision(&mut self, messages: &[AgentMessage]) -> Result<ModelDecision>;
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct AgentState {
    messages: Vec<AgentMessage>,
    iteration: usize,
    max_iterations: usize,
}

impl AgentState {
    pub(crate) fn new(prompt: String, max_iterations: usize) -> Result<Self> {
        if prompt.trim().is_empty() {
            bail!("agent prompt cannot be empty");
        }

        if max_iterations == 0 {
            bail!("max_iterations must be greater than 0");
        }

        Ok(Self {
            messages: vec![AgentMessage::User(prompt)],
            iteration: 0,
            max_iterations,
        })
    }

    fn begin_iteration(&mut self) -> Result<()> {
        if self.iteration >= self.max_iterations {
            bail!("agent reached maximum iterations");
        }
        self.iteration += 1;
        Ok(())
    }
}

pub(crate) async fn run_agent<M: AgentModel, T: ToolExecutor>(
    model: &mut M,
    prompt: String,
    max_iterations: usize,
    tool_executor: &mut T,
) -> Result<String> {
    let mut state = AgentState::new(prompt, max_iterations)?;
    loop {
        state.begin_iteration()?;
        let decision = model.next_decision(&state.messages).await?;
        match decision {
            ModelDecision::FinalAnswer(answer) => return Ok(answer),
            ModelDecision::ToolCall { id, name, input } => {
                let output = tool_executor
                    .execute(&name, &input)
                    .await
                    .with_context(|| format!("tool `{name}` execution failed"))?;
                state.messages.push(AgentMessage::ToolCall {
                    id: id.clone(),
                    name: name.clone(),
                    input,
                });

                let result = AgentMessage::ToolResult {
                    tool_call_id: id,
                    name: name,
                    output,
                };
                state.messages.push(result);
            }
        }
    }
}

pub(crate) trait ToolExecutor {
    async fn execute(&mut self, name: &str, input: &str) -> Result<String>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::BuiltinToolExecutor;
    use std::collections::VecDeque;

    struct FakeToolExecutor {
        output: String,
        calls: Vec<(String, String)>,
    }

    impl ToolExecutor for FakeToolExecutor {
        async fn execute(&mut self, name: &str, input: &str) -> Result<String> {
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
        let state = AgentState::new("hello".to_string(), 3).unwrap();
        assert_eq!(
            state.messages,
            vec![AgentMessage::User("hello".to_string())]
        );
        assert_eq!(state.iteration, 0);
        assert_eq!(state.max_iterations, 3);
    }

    #[test]
    fn rejects_blank_prompt() {
        let result = AgentState::new("   \n\t".to_string(), 3);
        let message = result.unwrap_err().to_string();
        assert_eq!(message, "agent prompt cannot be empty");
    }

    #[test]
    fn rejects_zero_max_iterations() {
        let result = AgentState::new("hello".to_string(), 0);
        let message = result.unwrap_err().to_string();
        assert_eq!(message, "max_iterations must be greater than 0");
    }

    #[test]
    fn allows_calls_up_to_max_iterations() {
        let mut state = AgentState::new("hello".to_string(), 3).unwrap();
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
            ModelDecision::ToolCall {
                id: "call-1".to_string(),
                name: "add".to_string(),
                input: "2,3".to_string(),
            },
            ModelDecision::FinalAnswer("5".to_string()),
        ]);

        let messages = vec![AgentMessage::User("add 2 and 3".to_string())];

        let first = model.next_decision(&messages).await.unwrap();

        assert_eq!(
            first,
            ModelDecision::ToolCall {
                id: "call-1".to_string(),
                name: "add".to_string(),
                input: "2,3".to_string(),
            }
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
            output: "5".to_string(),
            calls: Vec::new(),
        };

        let answer = run_agent(&mut model, "hello".to_string(), 3, &mut tool)
            .await
            .unwrap();
        assert_eq!(answer, "done");
        assert!(tool.calls.is_empty());
    }

    #[tokio::test]
    async fn fake_tool_executor_records_call_and_returns_output() {
        let mut tool = FakeToolExecutor {
            output: "5".to_string(),
            calls: Vec::new(),
        };
        let result = tool.execute("add", "2,3").await.unwrap();

        assert_eq!(result, "5");
        assert_eq!(tool.calls, vec![("add".to_string(), "2,3".to_string())]);
    }

    #[tokio::test]
    async fn agent_executes_tool_then_returns_final_answer() {
        let mut model = FakeModel::new(vec![
            ModelDecision::ToolCall {
                id: "call-1".to_string(),
                name: "add".to_string(),
                input: "2,3".to_string(),
            },
            ModelDecision::FinalAnswer("5".to_string()),
        ]);
        let mut tool = FakeToolExecutor {
            output: "5".to_string(),
            calls: Vec::new(),
        };

        let answer = run_agent(&mut model, "hello".to_string(), 2, &mut tool)
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
                    AgentMessage::ToolCall {
                        id: "call-1".to_string(),
                        name: "add".to_string(),
                        input: "2,3".to_string(),
                    },
                    AgentMessage::ToolResult {
                        tool_call_id: "call-1".to_string(),
                        name: "add".to_string(),
                        output: "5".to_string(),
                    }
                ],
            ]
        );
    }

    #[tokio::test]
    async fn stops_after_reaching_max_iterations() {
        let mut model = FakeModel::new(vec![ModelDecision::ToolCall {
            id: "call-1".to_string(),
            name: "add".to_string(),
            input: "2,3".to_string(),
        }]);

        let mut tool_executor = FakeToolExecutor {
            output: "5".to_string(),
            calls: Vec::new(),
        };

        let result = run_agent(&mut model, "hello".to_string(), 1, &mut tool_executor).await;

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
        async fn execute(&mut self, name: &str, input: &str) -> Result<String> {
            self.calls.push((name.to_string(), input.to_string()));

            bail!("tool execution failed");
        }
    }

    #[tokio::test]
    async fn returns_error_when_tool_execution_fails() {
        let mut model = FakeModel::new(vec![ModelDecision::ToolCall {
            id: "call-1".to_string(),
            name: "add".to_string(),
            input: "2,3".to_string(),
        }]);

        let mut tool_executor = FailingToolExecutor { calls: Vec::new() };

        let result = run_agent(&mut model, "hello".to_string(), 2, &mut tool_executor);

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
            ModelDecision::ToolCall {
                id: "call-1".to_string(),
                name: "add".to_string(),
                input: r#"{"a":2,"b":3}"#.to_string(),
            },
            ModelDecision::FinalAnswer("5".to_string()),
        ]);

        let mut tool_executor = BuiltinToolExecutor;

        let result = run_agent(&mut model, "add 2 and 3".to_string(), 2, &mut tool_executor)
            .await
            .unwrap();

        assert_eq!(result, "5");
        assert_eq!(model.received_messages.len(), 2);

        assert_eq!(
            model.received_messages[1],
            vec![
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
            ]
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
