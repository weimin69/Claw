mod cli;
mod commands;
mod config;
mod openai;

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
        Commands::Hello { name, age } => {
            commands::hello::execute(name, age)?;
        }
        Commands::Version => {
            commands::version::execute()?;
        }
        Commands::Echo { words, upper } => {
            commands::echo::execute(&words, upper)?;
        }
        Commands::Divide { dividend, divisor } => {
            commands::divide::execute(dividend, divisor)?;
        }
        Commands::Sum { numbers } => {
            commands::sum::execute(&numbers)?;
        }
        Commands::Repeat { words, times } => {
            commands::repeat::execute(&words, times)?;
        }
        Commands::ReadConfig { path } => {
            commands::read_config::execute(&path)?;
        }
        Commands::Wait { seconds } => {
            commands::wait::execute(seconds).await?;
        }
        Commands::Fetch { url } => {
            commands::fetch::execute(&url).await?;
        }
        Commands::Chat {
            config_path,
            prompt,
        } => {
            commands::chat::execute(&config_path, &prompt).await?;
        }
    }
    Ok(())
}
