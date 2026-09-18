mod cli;
mod ctx;
mod generate;
mod history;
mod model;
mod rbac;
mod scaffold;
mod search;
mod snippets;
mod tui;
mod util;

use clap::Parser;

use crate::cli::{Cli, Command};

fn main() -> anyhow::Result<()> {
    match Cli::parse().command {
        Command::Gen(args) => generate::run(*args),
        Command::Scaffold(args) => scaffold::run(args),
        Command::Rbac(args) => rbac::run(args),
        Command::Search(args) => search::run(args),
        Command::Ctx(args) => ctx::run(args),
        Command::Tui(args) => tui::run(args),
    }
}
