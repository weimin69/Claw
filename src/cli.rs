use clap::{Parser, Subcommand};

#[derive(Parser)] //Trait
#[command(version)]
pub struct Cli {
    #[command(subcommand)] // att
    pub command: Commands,
}
#[derive(Subcommand)] // 告知下面的类型的能力是子命令
pub enum Commands {
    ReadConfig {
        path: String,
    },
    Fetch {
        url: String,
        #[arg(long, default_value_t = 200, value_parser = parse_positive_usize)]
        max_chars: usize,
    },
    Chat {
        config_path: String,
        prompt: Option<String>,
    },
    Claw {
        config_path: String,
        prompt: Option<String>,
        /// Keep an in-memory conversation until /exit, EOF or Ctrl+C.
        #[arg(long)]
        interactive: bool,
        /// Save completed interactive turns to a NEW file; parent must exist.
        /// Contains potentially sensitive conversation and tool output.
        #[arg(long, requires = "interactive")]
        session_file: Option<std::path::PathBuf>,
        /// Resume and update an existing session; permissions come from this invocation.
        #[arg(long, requires = "interactive", conflicts_with = "session_file")]
        resume: Option<std::path::PathBuf>,
        /// Project directory available to repository tools.
        #[arg(long, default_value = ".")]
        workspace: std::path::PathBuf,
        /// Allow the agent to replace text in existing workspace files.
        #[arg(long)]
        allow_write: bool,
        /// Allow arbitrary command execution, including writes; this is not sandboxed.
        #[arg(long)]
        allow_exec: bool,
        /// Maximum model turns; omitted means no turn budget.
        #[arg(long, value_parser = parse_positive_usize)]
        max_iterations: Option<usize>,
        /// Maximum tool calls across the task; omitted means no tool budget.
        #[arg(long, value_parser = parse_positive_usize)]
        max_tool_calls: Option<usize>,
        /// Internal history JSON byte budget (not tokens or full HTTP size).
        #[arg(long, default_value_t = crate::agent::DEFAULT_MAX_CONTEXT_BYTES, value_parser = parse_positive_usize)]
        max_context_bytes: usize,
    },
}
fn parse_positive_usize(value: &str) -> Result<usize, String> {
    let parsed = value
        .parse::<usize>()
        .map_err(|_| "must be a positive integer".to_string())?;

    if parsed == 0 {
        return Err("must be greater than 0".to_string());
    }

    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claw_defaults_workspace_to_current_directory() {
        let cli = Cli::try_parse_from(["agent-cli-rust", "claw", "config.toml", "hello"]).unwrap();
        let Commands::Claw { workspace, .. } = cli.command else {
            panic!("expected claw command");
        };
        assert_eq!(workspace, std::path::PathBuf::from("."));
    }

    #[test]
    fn claw_accepts_explicit_workspace() {
        let cli = Cli::try_parse_from([
            "agent-cli-rust",
            "claw",
            "config.toml",
            "hello",
            "--workspace",
            "other-project",
        ])
        .unwrap();
        let Commands::Claw { workspace, .. } = cli.command else {
            panic!("expected claw command");
        };
        assert_eq!(workspace, std::path::PathBuf::from("other-project"));
    }
}
