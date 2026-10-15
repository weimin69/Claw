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
