use anyhow::{Result, bail};

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ModelDecision {
    FinalAnswer(String),
    ToolCall { name: String, input: String },
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum AgentMessage {
    User(String),
    ToolCall { name: String, input: String },
    ToolResult { name: String, output: String },
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

pub(crate) async fn run_agent<M: AgentModel>(
    model: &mut M,
    prompt: String,
    max_iterations: usize,
) -> Result<String> {
    let mut state = AgentState::new(prompt, max_iterations)?;
    loop {
        state.begin_iteration()?;
        let decision = model.next_decision(&state.messages).await?;
        match decision {
            ModelDecision::FinalAnswer(answer) => return Ok(answer),
            ModelDecision::ToolCall { .. } => {
                bail!("tool execution is not implemented");
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
    use std::collections::VecDeque;

    struct FakeModel {
        decisions: VecDeque<ModelDecision>,
    }

    impl FakeModel {
        fn new(decisions: Vec<ModelDecision>) -> Self {
            Self {
                decisions: decisions.into(),
            }
        }
    }

    impl AgentModel for FakeModel {
        async fn next_decision(&mut self, _messages: &[AgentMessage]) -> Result<ModelDecision> {
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

        let answer = run_agent(&mut model, "hello".to_string(), 3).await.unwrap();
        assert_eq!(answer, "done");
    }
}
