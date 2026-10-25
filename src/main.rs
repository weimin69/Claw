mod agent;
mod cli;
mod commands;
mod config;
mod openai_compatible;
mod session;
mod tools;
mod workspace;

use crate::cli::{Cli, Commands};
use anyhow::Result;
use clap::Parser;

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        if error.is::<commands::claw::Cancelled>() {
            eprintln!("{error}");
            std::process::exit(130);
        }
        eprintln!("error: {}", error);
        let causes: Vec<_> = error.chain().skip(1).collect();
        if !causes.is_empty() {
            eprintln!();
            eprintln!("caused by:");
        }

        for (index, cause) in causes.iter().enumerate() {
            eprintln!("     {}. {}", index + 1, cause);
        }
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::ReadConfig { path } => {
            commands::read_config::execute(&path)?;
        }

        Commands::Fetch { url, max_chars } => {
            commands::fetch::execute(&url, max_chars).await?;
        }
        Commands::Chat {
            config_path,
            prompt,
        } => {
            commands::chat::execute(&config_path, prompt).await?;
        }
        Commands::Claw {
            config_path,
            prompt,
            workspace,
            allow_write,
            allow_exec,
            max_iterations,
            max_tool_calls,
            max_context_bytes,
            interactive,
            session_file,
            resume,
        } => {
            commands::claw::execute(
                &config_path,
                prompt,
                commands::claw::ClawOptions {
                    workspace_path: &workspace,
                    allow_write,
                    allow_exec,
                    max_iterations,
                    max_tool_calls,
                    max_context_bytes,
                    interactive,
                    session_file: session_file.as_deref(),
                    resume: resume.as_deref(),
                },
            )
            .await?;
        }
    }
    Ok(())
}
