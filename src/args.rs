use clap::Parser;

#[derive(Debug, Parser, Clone)]
pub struct Args {
    #[clap(long)]
    pub registry: String,
    #[clap(long)]
    pub retention: u32,
    #[clap(long)]
    pub debug: bool,
    #[clap(long)]
    pub trace: bool,
    #[clap(long)]
    pub dry_run: bool,
}

impl Args {
    pub fn new() -> Self {
        Self::parse()
    }
}