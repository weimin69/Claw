mod agent;
mod cli;
mod commands;
mod config;
mod openai_compatible;
mod tools;

use crate::cli::{Cli, Commands};
use anyhow::Result;
use clap::Parser;

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
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
            max_iterations,
        } => {
            commands::claw::execute(&config_path, prompt, max_iterations).await?;
        }
    }
    Ok(())
}
