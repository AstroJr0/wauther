mod cli;
mod concave;
mod core;
mod tui;
mod ui;

use std::process::ExitCode;

use clap::Parser;

use cli::{Cli, Next};

fn main() -> ExitCode {
    match cli::run(Cli::parse()) {
        Ok(Next::Done) => ExitCode::SUCCESS,
        Ok(Next::Tui(start)) => finish(tui::run(start)),
        Ok(Next::Gui) => match core::Vault::open_default() {
            Ok(vault) => finish(ui::run(vault)),
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
