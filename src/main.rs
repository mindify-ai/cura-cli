use clap::Parser;
use cura::{cli::Cli, commands};

fn main() {
    if let Err(error) = commands::run(Cli::parse()) {
        eprintln!("\x1b[31merror:\x1b[0m {error:#}");
        std::process::exit(1);
    }
}
