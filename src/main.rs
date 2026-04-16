mod command_tree;
mod log_tree;
mod model;
mod shell_out;
mod terminal;
mod update;
mod view;

use crate::model::{Model, State};
use crate::update::update;
use crate::view::view;

use anyhow::Result;
use clap::Parser;
use model::DEFAULT_REVSET;
use shell_out::JjCommand;
use terminal::Term;

#[derive(Parser, Debug)]
#[command(version, about = "Jjdag: A TUI to manipulate the Jujutsu DAG")]
struct Args {
    /// Path to repository to operate on
    #[arg(short = 'R', long, default_value = ".")]
    repository: String,

    /// Which revisions to show
    #[arg(short = 'r', long, value_name = "REVSETS", default_value = DEFAULT_REVSET)]
    revisions: String,
}

fn main() {
    let result = run();
    if let Err(err) = result {
        // Avoids a redundant message "Error: Error:"
        eprintln!("{err}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args = Args::parse();
    let repository = JjCommand::jj_ensure_valid_repo(&args.repository)?;
    let model = Model::new(repository, args.revisions)?;

    let terminal = terminal::init_terminal()?;
    let result = tui_loop(model, terminal);
    terminal::relinquish_terminal()?;

    result
}

fn tui_loop(mut model: Model, terminal: Term) -> Result<()> {
    while model.state != State::Quit {
        terminal.borrow_mut().draw(|f| view(&mut model, f))?;
        update(terminal.clone(), &mut model)?;
    }
    Ok(())
}
