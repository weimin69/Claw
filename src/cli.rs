use clap::{Parser, Subcommand};

#[derive(Parser)] //Trait

pub struct Cli {
    #[command(subcommand)] // att
    pub command: Commands,
}
#[derive(Subcommand)] // 告知下面的类型的能力是子命令
pub enum Commands {
    Hello {
        name: String,
        age: u32,
    },
    Version,
    Echo {
        #[arg(required = true)]
        words: Vec<String>,

        #[arg(long)]
        upper: bool,
    },
    Divide {
        dividend: f64,
        divisor: f64,
    },
    Sum {
        numbers: Vec<f64>,
    },
    Repeat {
        #[arg(required = true)]
        words: Vec<String>,

        #[arg(long)]
        times: u32,
    },
    ReadConfig {
        path: String,
    },
    Wait {
        seconds: u64,
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
    ParallelWait {
        first_seconds: u64,
        second_seconds: u64,
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
