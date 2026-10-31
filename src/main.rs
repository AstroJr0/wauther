mod backend;
mod cli;
mod core;
mod tui;

use std::process::ExitCode;

use clap::Parser;

use cli::{Cli, Next};

fn main() -> ExitCode {
    match cli::run(Cli::parse()) {
        Ok(Next::Done) => ExitCode::SUCCESS,
        Ok(Next::Tui(start)) => finish(tui::run(start)),
        Ok(Next::Gui) => match core::Vault::open_default() {
            Ok(vault) => ExitCode::from(u8::try_from(backend::run(vault)).unwrap_or(1)),
            Err(e) => finish(Err(e)),
        },
        Err(e) => {
            eprintln!("wauther: {e}");
            ExitCode::FAILURE
        }
    }
}

fn finish<E: std::fmt::Display>(result: Result<(), E>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("wauther: {e}");
            ExitCode::FAILURE
        }
    }
}
