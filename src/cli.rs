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
    },
    Chat {
        config_path: String,
        prompt: String,
    },
}
