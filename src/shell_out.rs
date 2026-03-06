use crate::model::GlobalArgs;
use crate::terminal::{self, Term};
use anyhow::{Result, anyhow};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use regex::Regex;
use std::{
    env,
    io::{Read, Write},
    process::Command,
};

#[derive(Debug)]
pub struct JjCommand {
    args: Vec<String>,
    global_args: GlobalArgs,
    interactive_term: Option<Term>,
    return_output: ReturnOutput,
    sync: bool,
}

impl JjCommand {
    fn _new(
        args: &[&str],
        global_args: GlobalArgs,
        interactive_term: Option<Term>,
        return_output: ReturnOutput,
    ) -> Self {
        Self {
            args: args.iter().map(|a| a.to_string()).collect(),
            global_args,
            interactive_term,
            return_output,
            sync: true,
        }
    }

    fn _new_skip_sync(
        args: &[&str],
        global_args: GlobalArgs,
        interactive_term: Option<Term>,
        return_output: ReturnOutput,
    ) -> Self {
        Self {
            args: args.iter().map(|a| a.to_string()).collect(),
            global_args,
            interactive_term,
            return_output,
            sync: false,
        }
    }

    pub fn sync(&self) -> bool {
        self.sync
    }

    pub fn to_lines(&self) -> Vec<Line<'static>> {
        let line = Line::from(vec![
            Span::styled("❯", Style::default().fg(Color::Yellow)),
            Span::raw(" jj "),
            Span::raw(self.args.join(" ")),
        ]);
        let blank_line = Line::raw("");
        vec![line, blank_line]
    }

    pub fn run(&self) -> Result<String, JjCommandError> {
        let output = match &self.interactive_term {
            None => self.run_noninteractive(),
            Some(term) => self.run_interactive(term),
        }?;
        match self.return_output {
            ReturnOutput::Stdout => Ok(output.stdout),
            ReturnOutput::Stderr => Ok(output.stderr),
        }
    }

    fn run_noninteractive(&self) -> Result<JjCommandOutput, JjCommandError> {
        let mut command = self.base_command();
        command.args(self.args.clone());
        let output = command.output().map_err(JjCommandError::new_other)?;

        let stderr = String::from_utf8_lossy(&output.stderr).into();
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout).into();
            Ok(JjCommandOutput { stdout, stderr })
        } else {
            Err(JjCommandError::new_failed(stderr))
        }
    }

    fn run_interactive(&self, term: &Term) -> Result<JjCommandOutput, JjCommandError> {
        let mut command = self.base_command();
        command.args(self.args.clone());
        command.stderr(std::process::Stdio::piped());

        terminal::relinquish_terminal().map_err(JjCommandError::new_other)?;

        let mut child = command.spawn().map_err(JjCommandError::new_other)?;
        let mut stderr_handle = child
            .stderr
            .take()
            .ok_or_else(|| JjCommandError::new_other(anyhow!("No stderr handle")))?;
        let mut buf = Vec::new();
        stderr_handle
            .read_to_end(&mut buf)
            .map_err(JjCommandError::new_other)?;
        let stderr = strip_non_style_ansi(&String::from_utf8_lossy(&buf));
        let status = child.wait().map_err(JjCommandError::new_other)?;

        terminal::takeover_terminal(term).map_err(JjCommandError::new_other)?;

        if status.success() {
            Ok(JjCommandOutput {
                stdout: "".to_string(),
                stderr,
            })
        } else {
            Err(JjCommandError::new_failed(stderr))
        }
    }

    fn base_command(&self) -> Command {
        let mut command = Command::new("jj");
        let args = [
            "--color",
            "always",
            "--config",
            "ui.pager=:builtin",
            "--config",
            "ui.streampager.interface=full-screen-clear-output",
            "--config",
            "template-aliases.\"format_short_change_id(id)\"=format_short_id(id)",
            "--config",
            "template-aliases.\"format_short_id(id)\"=id.shortest(8)",
            "--config",
            r#"template-aliases."format_short_signature(signature)"="coalesce(signature.email(), email_placeholder)""#,
            "--config",
            r#"template-aliases."format_timestamp(timestamp)"='timestamp.local().format("%Y-%m-%d %H:%M:%S")'"#,
            "--config",
            r#"templates.log_node=
                coalesce(
                  if(!self, label("elided", "~")),
                  label(
                    separate(" ",
                      if(current_working_copy, "working_copy"),
                      if(immutable, "immutable"),
                      if(conflict, "conflict"),
                    ),
                    coalesce(
                      if(current_working_copy, "@"),
                      if(root, "┴"),
                      if(immutable, "●"),
                      if(conflict, "⊗"),
                      "○",
                    )
                  )
                )
            "#,
            "--repository",
            &self.global_args.repository,
        ];
        command.args(args);

        if self.global_args.ignore_immutable {
            command.arg("--ignore-immutable");
        }

        command
    }

    pub fn log(revset: &str, global_args: GlobalArgs) -> Self {
        let args = [
            "log",
            "--template",
            "builtin_log_compact",
            "--revisions",
            revset,
        ];
        Self::_new(&args, global_args, None, ReturnOutput::Stdout)
    }

    pub fn diff_summary(change_id: &str, global_args: GlobalArgs) -> Self {
        let args = ["diff", "--summary", "--revisions", change_id];
        Self::_new(&args, global_args, None, ReturnOutput::Stdout)
    }

    pub fn diff_file(change_id: &str, file: &str, global_args: GlobalArgs) -> Self {
        let args = ["diff", "--color-words", "--revisions", change_id, file];
        Self::_new(&args, global_args, None, ReturnOutput::Stdout)
    }

    pub fn diff_file_interactive(
        change_id: &str,
        file: &str,
        global_args: GlobalArgs,
        term: Term,
    ) -> Self {
        let args = ["diff", "--revisions", change_id, file];
        Self::_new_skip_sync(&args, global_args, Some(term), ReturnOutput::Stderr)
    }

    pub fn diff_from_to_interactive(
        from: &str,
        to: &str,
        file: Option<&str>,
        global_args: GlobalArgs,
        term: Term,
    ) -> Self {
        let mut args = vec!["diff", "--from", from, "--to", to];
        if let Some(file) = file {
            args.push(file);
        }
        Self::_new_skip_sync(&args, global_args, Some(term), ReturnOutput::Stderr)
    }

    pub fn describe(change_id: &str, global_args: GlobalArgs, term: Term) -> Self {
        let args = ["describe", change_id];
        Self::_new(&args, global_args, Some(term), ReturnOutput::Stderr)
    }

    pub fn duplicate(
        change_id: &str,
        destination_type: Option<&str>,
        destination: Option<&str>,
        global_args: GlobalArgs,
    ) -> Self {
        let mut args = vec!["duplicate", change_id];
        if let (Some(destination_type), Some(destination)) = (destination_type, destination) {
            args.push(destination_type);
            args.push(destination);
        }
        Self::_new(&args, global_args, None, ReturnOutput::Stderr)
    }

    pub fn new(target: &str, flags: &[&str], global_args: GlobalArgs) -> Self {
        let mut args = vec!["new"];
        args.extend_from_slice(flags);
        args.push(target);
        Self::_new(&args, global_args, None, ReturnOutput::Stderr)
    }

    pub fn parallelize(revset: &str, global_args: GlobalArgs) -> Self {
        let args = ["parallelize", revset];
        Self::_new(&args, global_args, None, ReturnOutput::Stderr)
    }

    pub fn next_prev(
        direction: &str,
        mode: Option<&str>,
        offset: Option<&str>,
        global_args: GlobalArgs,
    ) -> Self {
        let mut args = vec![direction];
        if let Some(mode) = mode {
            args.push(mode);
        }
        if let Some(offset) = offset {
            args.push(offset);
        }
        Self::_new(&args, global_args, None, ReturnOutput::Stderr)
    }

    pub fn abandon(change_id: &str, mode: Option<&str>, global_args: GlobalArgs) -> Self {
        let mut args = vec!["abandon"];
        if let Some(mode) = mode {
            args.push(mode);
        }
        args.push(change_id);
        Self::_new(&args, global_args, None, ReturnOutput::Stderr)
    }

    pub fn absorb(
        from_change_id: &str,
        maybe_into_change_id: Option<&str>,
        maybe_file_path: Option<&str>,
        global_args: GlobalArgs,
    ) -> Self {
        let mut args = vec!["absorb", "--from", from_change_id];
        if let Some(into_change_id) = maybe_into_change_id {
            args.push("--into");
            args.push(into_change_id);
        }
        if let Some(file_path) = maybe_file_path {
            args.push(file_path);
        }
        Self::_new(&args, global_args, None, ReturnOutput::Stderr)
    }

    pub fn revert(
        revision: &str,
        destination_type: &str,
        destination: &str,
        global_args: GlobalArgs,
    ) -> Self {
        let args = ["revert", "-r", revision, destination_type, destination];
        Self::_new(&args, global_args, None, ReturnOutput::Stderr)
    }

    pub fn sign(action: &str, revset: &str, global_args: GlobalArgs) -> Self {
        let args = [action, "-r", revset];
        Self::_new(&args, global_args, None, ReturnOutput::Stderr)
    }

    pub fn show(change_id: &str, global_args: GlobalArgs, term: Term) -> Self {
        let args = ["show", change_id];
        Self::_new_skip_sync(&args, global_args, Some(term), ReturnOutput::Stderr)
    }

    pub fn status(global_args: GlobalArgs, term: Term) -> Self {
        let args = ["status"];
        Self::_new_skip_sync(&args, global_args, Some(term), ReturnOutput::Stderr)
    }

    pub fn simplify_parents(revision: &str, mode: &str, global_args: GlobalArgs) -> Self {
        let args = ["simplify-parents", mode, revision];
        Self::_new(&args, global_args, None, ReturnOutput::Stderr)
    }

    pub fn undo(global_args: GlobalArgs) -> Self {
        let args = ["undo"];
        Self::_new(&args, global_args, None, ReturnOutput::Stderr)
    }

    pub fn redo(global_args: GlobalArgs) -> Self {
        let args = ["redo"];
        Self::_new(&args, global_args, None, ReturnOutput::Stderr)
    }

    pub fn commit(maybe_file_path: Option<&str>, global_args: GlobalArgs, term: Term) -> Self {
        let mut args = vec!["commit"];
        if let Some(file_path) = maybe_file_path {
            args.push(file_path);
        }
        Self::_new(&args, global_args, Some(term), ReturnOutput::Stderr)
    }

    pub fn rebase(
        source_type: &str,
        source: &str,
        destination_type: &str,
        destination: &str,
        global_args: GlobalArgs,
    ) -> Self {
        let args = vec!["rebase", source_type, source, destination_type, destination];
        Self::_new(&args, global_args, None, ReturnOutput::Stderr)
    }

    pub fn resolve(
        change_id: &str,
        maybe_file_path: Option<&str>,
        global_args: GlobalArgs,
        term: Term,
    ) -> Self {
        let mut args = vec!["resolve", "-r", change_id];
        if let Some(file_path) = maybe_file_path {
            args.push(file_path);
        }
        Self::_new(&args, global_args, Some(term), ReturnOutput::Stderr)
    }

    pub fn restore(flags: &[&str], maybe_file_path: Option<&str>, global_args: GlobalArgs) -> Self {
        let mut args = vec!["restore"];
        args.extend_from_slice(flags);
        if let Some(file_path) = maybe_file_path {
            args.push(file_path);
        }
        Self::_new(&args, global_args, None, ReturnOutput::Stderr)
    }

    pub fn squash_noninteractive(
        change_id: &str,
        maybe_file_path: Option<&str>,
        global_args: GlobalArgs,
    ) -> Self {
        let mut args = vec!["squash", "--revision", change_id];
        if let Some(file_path) = maybe_file_path {
            args.push(file_path);
        }
        Self::_new(&args, global_args, None, ReturnOutput::Stderr)
    }

    pub fn squash_interactive(
        change_id: &str,
        maybe_file_path: Option<&str>,
        global_args: GlobalArgs,
        term: Term,
    ) -> Self {
        let mut args = vec!["squash", "--revision", change_id];
        if let Some(file_path) = maybe_file_path {
            args.push(file_path);
        }
        Self::_new(&args, global_args, Some(term), ReturnOutput::Stderr)
    }

    pub fn squash_into_interactive(
        from_change_id: &str,
        into_change_id: &str,
        maybe_file_path: Option<&str>,
        global_args: GlobalArgs,
        term: Term,
    ) -> Self {
        let mut args = vec!["squash", "--from", from_change_id, "--into", into_change_id];
        if let Some(file_path) = maybe_file_path {
            args.push(file_path);
        }
        Self::_new(&args, global_args, Some(term), ReturnOutput::Stderr)
    }

    pub fn edit(change_id: &str, global_args: GlobalArgs) -> Self {
        let args = ["edit", change_id];
        Self::_new(&args, global_args, None, ReturnOutput::Stderr)
    }

    pub fn evolog(change_id: &str, patch: bool, global_args: GlobalArgs, term: Term) -> Self {
        let mut args = vec!["evolog", "-r", change_id];
        if patch {
            args.push("--patch");
        }
        Self::_new_skip_sync(&args, global_args, Some(term), ReturnOutput::Stderr)
    }

    pub fn interdiff(
        from: &str,
        to: &str,
        maybe_file_path: Option<&str>,
        global_args: GlobalArgs,
        term: Term,
    ) -> Self {
        let mut args = vec!["interdiff", "--from", from, "--to", to];
        if let Some(path) = maybe_file_path {
            args.push(path);
        }
        Self::_new_skip_sync(&args, global_args, Some(term), ReturnOutput::Stderr)
    }

    pub fn file_track(file_path: &str, global_args: GlobalArgs) -> Self {
        let args = ["file", "track", file_path];
        Self::_new(&args, global_args, None, ReturnOutput::Stderr)
    }

    pub fn file_untrack(file_path: &str, global_args: GlobalArgs) -> Self {
        let args = ["file", "untrack", file_path];
        Self::_new(&args, global_args, None, ReturnOutput::Stderr)
    }

    pub fn metaedit(
        change_id: &str,
        flag: &str,
        value: Option<&str>,
        global_args: GlobalArgs,
    ) -> Self {
        let mut args = vec!["metaedit", flag];
        if let Some(value) = value {
            args.push(value);
        }
        args.push(change_id);
        Self::_new(&args, global_args, None, ReturnOutput::Stderr)
    }

    pub fn git_fetch(flag: Option<&str>, value: Option<&str>, global_args: GlobalArgs) -> Self {
        let mut args = vec!["git", "fetch"];
        if let Some(flag) = flag {
            args.push(flag);
        }
        if let Some(value) = value {
            args.push(value);
        }
        Self::_new(&args, global_args, None, ReturnOutput::Stderr)
    }

    pub fn git_push(flag: Option<&str>, value: Option<&str>, global_args: GlobalArgs) -> Self {
        let mut args = vec!["git", "push"];
        if let Some(flag) = flag {
            args.push(flag);
        }
        if let Some(value) = value {
            args.push(value);
        }
        Self::_new(&args, global_args, None, ReturnOutput::Stderr)
    }

    pub fn bookmark_create(bookmark_names: &str, change_id: &str, global_args: GlobalArgs) -> Self {
        let args = [
            "bookmark",
            "create",
            "--revision",
            change_id,
            bookmark_names,
        ];
        Self::_new(&args, global_args, None, ReturnOutput::Stderr)
    }

    pub fn bookmark_delete(bookmark_names: &str, global_args: GlobalArgs) -> Self {
        let args = ["bookmark", "delete", bookmark_names];
        Self::_new(&args, global_args, None, ReturnOutput::Stderr)
    }

    pub fn bookmark_forget(
        bookmark_names: &str,
        include_remotes: bool,
        global_args: GlobalArgs,
    ) -> Self {
        let mut args = vec!["bookmark", "forget"];
        if include_remotes {
            args.push("--include-remotes");
        }
        args.push(bookmark_names);
        Self::_new(&args, global_args, None, ReturnOutput::Stderr)
    }

    pub fn bookmark_move(
        from_change_id: &str,
        to_change_id: &str,
        allow_backwards: bool,
        global_args: GlobalArgs,
    ) -> Self {
        let mut args = vec![
            "bookmark",
            "move",
            "--from",
            from_change_id,
            "--to",
            to_change_id,
        ];
        if allow_backwards {
            args.push("--allow-backwards");
        }
        Self::_new(&args, global_args, None, ReturnOutput::Stderr)
    }

    pub fn bookmark_rename(
        old_bookmark_name: &str,
        new_bookmark_name: &str,
        global_args: GlobalArgs,
    ) -> Self {
        let args = ["bookmark", "rename", old_bookmark_name, new_bookmark_name];
        Self::_new(&args, global_args, None, ReturnOutput::Stderr)
    }

    pub fn bookmark_set(bookmark_names: &str, change_id: &str, global_args: GlobalArgs) -> Self {
        let args = ["bookmark", "set", bookmark_names, "--revision", change_id];
        Self::_new(&args, global_args, None, ReturnOutput::Stderr)
    }

    pub fn bookmark_track(bookmark_at_remote: &str, global_args: GlobalArgs) -> Self {
        let args = ["bookmark", "track", bookmark_at_remote];
        Self::_new(&args, global_args, None, ReturnOutput::Stderr)
    }

    pub fn bookmark_untrack(bookmark_at_remote: &str, global_args: GlobalArgs) -> Self {
        let args = ["bookmark", "untrack", bookmark_at_remote];
        Self::_new(&args, global_args, None, ReturnOutput::Stderr)
    }

    pub fn ensure_valid_repo(repository: &str) -> Result<String, JjCommandError> {
        let args = [
            "--repository",
            repository,
            "workspace",
            "root",
            "--color",
            "always",
        ];
        let output = Command::new("jj")
            .args(args)
            .output()
            .map_err(JjCommandError::new_other)?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout)
                .to_string()
                .trim()
                .to_string())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr).into();
            Err(JjCommandError::new_failed(stderr))
        }
    }
}

#[derive(Debug)]
enum ReturnOutput {
    Stdout,
    Stderr,
}

#[derive(Debug)]
pub enum JjCommandError {
    Failed { stderr: String },
    Other { err: anyhow::Error },
}

impl JjCommandError {
    fn new_failed(stderr: String) -> Self {
        Self::Failed {
            stderr: stderr.trim().to_string(),
        }
    }

    fn new_other(err: impl Into<anyhow::Error>) -> Self {
        Self::Other { err: err.into() }
    }
}

impl std::fmt::Display for JjCommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Failed { stderr } => {
                write!(f, "{stderr}")
            }
            Self::Other { err } => err.fmt(f),
        }
    }
}

impl std::error::Error for JjCommandError {}

#[derive(Debug)]
struct JjCommandOutput {
    stdout: String,
    stderr: String,
}

pub fn get_input_from_editor(
    interactive_term: Term,
    starting_text: Option<&str>,
    help_text: Option<&str>,
) -> Result<Option<String>> {
    // Create temp file
    let mut temp_file = tempfile::Builder::new()
        .suffix(".jjdescription")
        .tempfile()?;
    if let Some(text) = starting_text {
        writeln!(temp_file, "{text}")?;
        temp_file.flush()?;
    }
    if let Some(text) = help_text {
        writeln!(temp_file, "\n\nJJ: {text}")?;
        writeln!(
            temp_file,
            "JJ: Lines starting with \"JJ:\" (like this one) will be removed."
        )?;

        temp_file.flush()?;
    }
    let temp_path = temp_file.path().to_path_buf();

    // Open editor in temp file
    let editor = env::var("EDITOR").unwrap_or_else(|_| "vim".to_string());
    terminal::relinquish_terminal()?;
    let status = Command::new(&editor).arg(&temp_path).status()?;
    terminal::takeover_terminal(&interactive_term)?;
    if !status.success() {
        anyhow::bail!("Editor exited with non-zero status");
    }

    // Remove all lines starting with "JJ: "
    let contents = std::fs::read_to_string(&temp_path)?;
    let contents: String = contents
        .lines()
        .filter(|line| !line.starts_with("JJ:"))
        .collect::<Vec<&str>>()
        .join("\n")
        .trim()
        .to_string();
    if contents.is_empty() {
        Ok(None)
    } else {
        Ok(Some(contents))
    }
}

pub fn open_file_in_editor(interactive_term: Term, file_path: &str) -> Result<()> {
    let editor = env::var("EDITOR").unwrap_or_else(|_| "vim".to_string());
    terminal::relinquish_terminal()?;
    let status = Command::new(&editor).arg(file_path).status()?;
    terminal::takeover_terminal(&interactive_term)?;
    if !status.success() {
        anyhow::bail!("'{editor}' exited with status {status} for '{file_path}'");
    }
    Ok(())
}

fn strip_non_style_ansi(str: &str) -> String {
    let non_style_ansi_regex =
        Regex::new(r"\x1b(\[[0-9;?]*[ -/]*([@-l]|[n-~])|\].*?(\x07|\x1b\\)|P.*?\x1b\\)").unwrap();
    non_style_ansi_regex.replace_all(str, "").to_string()
}
