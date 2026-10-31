mod backend;
mod cli;

use std::process::ExitCode;

use clap::Parser;

use cli::Cli;

fn main() -> ExitCode {
    match Cli::parse().run() {
        Ok(None) => ExitCode::SUCCESS,
        Ok(Some(vault)) => ExitCode::from(u8::try_from(backend::run(vault)).unwrap_or(1)),
        Err(e) => {
            eprintln!("wauther: {e}");
            ExitCode::FAILURE
        }
    }
}
