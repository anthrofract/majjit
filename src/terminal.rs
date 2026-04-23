use anyhow::Result;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::{
    cell::RefCell,
    io::{Stdout, stdout},
    panic,
    rc::Rc,
};
use terminal_colorsaurus::{QueryOptions, ThemeMode};

pub type Term = Rc<RefCell<Terminal<CrosstermBackend<Stdout>>>>;

pub fn init_terminal() -> Result<Term> {
    install_panic_hook();
    enable_raw_mode()?;
    execute!(stdout(), EnterAlternateScreen, EnableMouseCapture)?;
    let terminal = Rc::new(RefCell::new(Terminal::new(
        CrosstermBackend::new(stdout()),
    )?));
    Ok(terminal)
}

pub fn takeover_terminal(terminal: &Term) -> Result<()> {
    enable_raw_mode()?;
    execute!(stdout(), EnterAlternateScreen, EnableMouseCapture)?;
    terminal.borrow_mut().clear()?;
    Ok(())
}

pub fn relinquish_terminal() -> Result<()> {
    execute!(stdout(), LeaveAlternateScreen, DisableMouseCapture)?;
    disable_raw_mode()?;
    Ok(())
}

pub fn install_panic_hook() {
    let original_hook = panic::take_hook();
    panic::set_hook(Box::new(move |panic_info| {
        relinquish_terminal().unwrap();
        original_hook(panic_info);
    }));
}

pub fn detect_terminal_theme() -> Result<ThemeMode> {
    Ok(terminal_colorsaurus::theme_mode(QueryOptions::default())?)
}
