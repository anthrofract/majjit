use anyhow::Result;
use crossterm::{
    cursor::Show,
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Frame, backend::CrosstermBackend};
use std::{
    io::{Stdout, stdout},
    panic,
};
use terminal_colorsaurus::{QueryOptions, ThemeMode};

pub struct Terminal {
    backend: ratatui::Terminal<CrosstermBackend<Stdout>>,
}

impl Terminal {
    pub fn new() -> Result<Self> {
        install_panic_hook();
        let mut terminal = Self {
            backend: ratatui::Terminal::new(CrosstermBackend::new(stdout()))?,
        };
        terminal.enter()?;
        Ok(terminal)
    }

    pub fn draw(&mut self, render: impl FnOnce(&mut Frame)) -> Result<()> {
        self.backend.draw(render)?;
        Ok(())
    }

    pub fn detect_theme(&self) -> Result<ThemeMode> {
        Ok(terminal_colorsaurus::theme_mode(QueryOptions::default())?)
    }

    pub fn with_suspended<T>(&mut self, operation: impl FnOnce() -> T) -> Result<T> {
        if let Err(err) = leave_terminal() {
            self.resume()?;
            return Err(err.into());
        }
        let result = operation();
        self.resume()?;
        Ok(result)
    }

    fn enter(&mut self) -> Result<()> {
        let result = enable_raw_mode()
            .and_then(|()| execute!(stdout(), EnterAlternateScreen, EnableMouseCapture));
        if result.is_err() {
            let _ = leave_terminal();
        }
        Ok(result?)
    }

    fn resume(&mut self) -> Result<()> {
        self.enter()?;
        if let Err(err) = self.backend.clear() {
            let _ = leave_terminal();
            return Err(err.into());
        }
        Ok(())
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = leave_terminal();
    }
}

fn leave_terminal() -> std::io::Result<()> {
    // Do not short-circuit: even a broken output stream must not leave raw mode on.
    let cursor = execute!(stdout(), Show);
    let mouse = execute!(stdout(), DisableMouseCapture);
    let screen = execute!(stdout(), LeaveAlternateScreen);
    let raw = disable_raw_mode();
    cursor.and(mouse).and(screen).and(raw)
}

fn install_panic_hook() {
    let original_hook = panic::take_hook();
    panic::set_hook(Box::new(move |panic_info| {
        let _ = leave_terminal();
        original_hook(panic_info);
    }));
}
