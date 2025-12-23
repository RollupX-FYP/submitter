use anyhow::Result;
use clap::Parser;
use dotenvy::dotenv;
use std::path::PathBuf;
use submitter_rs::script;

#[derive(Parser, Debug)]
struct Args {
    #[arg(long)]
    config: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();
    tracing_subscriber::fmt::init();

    let args = Args::parse();
    script::run(args.config).await
}
