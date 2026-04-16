use crate::model::GlobalArgs;
use crate::shell_out::JjCommand;
use ansi_to_tui::IntoText;
use anyhow::{Error, Result, anyhow, bail};
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
};
use regex::Regex;
use std::fmt;

#[derive(Debug)]
pub struct JjLog {
    pub log_tree: Vec<CommitOrText>,
}

impl JjLog {
    pub fn new() -> Result<Self> {
        Ok(JjLog {
            log_tree: Vec::new(),
        })
    }

    pub fn load_log_tree(&mut self, global_args: &GlobalArgs, revset: &str) -> Result<()> {
        self.log_tree = CommitOrText::load_all(global_args, revset)?;
        Ok(())
    }

    pub fn flatten_log(&mut self) -> Result<(Vec<Text<'static>>, Vec<TreePosition>)> {
        let mut log_list = Vec::new();
        let mut log_list_tree_positions = Vec::new();

        for (commit_or_text_idx, commit_or_text) in self.log_tree.iter_mut().enumerate() {
            commit_or_text.flatten(
                vec![commit_or_text_idx],
                &mut log_list,
                &mut log_list_tree_positions,
            )?;
        }

        Ok((log_list, log_list_tree_positions))
    }

    pub fn get_tree_node(&mut self, tree_pos: &TreePosition) -> Result<&mut dyn LogTreeNode> {
        // Traverse to commit
        let commit_or_text = &mut self.log_tree[tree_pos[COMMIT_OR_TEXT_IDX]];
        let commit = match commit_or_text {
            CommitOrText::InfoText(info_text) => {
                return Ok(info_text);
            }
            CommitOrText::Commit(commit) => commit,
        };

        let file_diff_idx = if tree_pos.len() <= FILE_DIFF_IDX {
            return Ok(commit);
        } else {
            tree_pos[FILE_DIFF_IDX]
        };

        // Traverse to file diff
        if !commit.loaded {
            bail!("Trying to get unloaded file diffs for commit");
        }
        let file_diff = &mut commit.file_diffs[file_diff_idx];
        let diff_hunk_idx = if tree_pos.len() <= DIFF_HUNK_IDX {
            return Ok(file_diff);
        } else {
            tree_pos[DIFF_HUNK_IDX]
        };

        // Traverse to diff hunk
        if !file_diff.loaded {
            bail!("Trying to get unloaded diff hunks for file diff");
        }
        let diff_hunk = &mut file_diff.diff_hunks[diff_hunk_idx];
        let diff_hunk_line_idx = if tree_pos.len() <= DIFF_HUNK_LINE_IDX {
            return Ok(diff_hunk);
        } else {
            tree_pos[DIFF_HUNK_LINE_IDX]
        };

        // Traverse to diff hunk line
        let diff_hunk_line = &mut diff_hunk.diff_hunk_lines[diff_hunk_line_idx];
        Ok(diff_hunk_line)
    }

    pub fn get_tree_commit(&self, tree_pos: &TreePosition) -> Option<&Commit> {
        let commit_or_text = &self.log_tree[tree_pos[COMMIT_OR_TEXT_IDX]];
        match commit_or_text {
            CommitOrText::InfoText(_) => None,
            CommitOrText::Commit(commit) => Some(commit),
        }
    }

    pub fn get_tree_file_diff(&self, tree_pos: &TreePosition) -> Option<&FileDiff> {
        if tree_pos.len() <= FILE_DIFF_IDX {
            return None;
        }
        let commit = self.get_tree_commit(tree_pos)?;
        Some(&commit.file_diffs[tree_pos[FILE_DIFF_IDX]])
    }

    pub fn get_current_commit(&self) -> Option<&Commit> {
        // TODO: cache this instead of looping each time?
        self.log_tree.iter().find_map(|item| match item {
            CommitOrText::Commit(commit) if commit.current_working_copy => Some(commit),
            _ => None,
        })
    }

    pub fn toggle_fold(
        &mut self,
        global_args: &GlobalArgs,
        tree_pos: &TreePosition,
    ) -> Result<usize> {
        let mut tree_pos = tree_pos.clone();
        tree_pos.truncate(DIFF_HUNK_IDX + 1);
        let node = self.get_tree_node(&tree_pos)?;
        node.toggle_fold(global_args)?;
        Ok(node.flat_log_idx())
    }
}

pub trait LogTreeNode {
    fn render(&self) -> Result<Text<'static>>;
    fn flatten(
        &mut self,
        tree_pos: TreePosition,
        log_list: &mut Vec<Text<'static>>,
        log_list_tree_positions: &mut Vec<TreePosition>,
    ) -> Result<()>;
    fn flat_log_idx(&self) -> usize;
    fn children(&self) -> Vec<&dyn LogTreeNode>;
    fn toggle_fold(&mut self, global_args: &GlobalArgs) -> Result<()>;
}

pub type TreePosition = Vec<usize>;
const COMMIT_OR_TEXT_IDX: usize = 0;
const FILE_DIFF_IDX: usize = 1;
const DIFF_HUNK_IDX: usize = 2;
pub const DIFF_HUNK_LINE_IDX: usize = 3;

pub fn get_parent_tree_position(tree_pos: &TreePosition) -> Option<TreePosition> {
    let mut tree_pos = tree_pos.clone();
    if tree_pos.len() > 1 {
        tree_pos.pop();
        Some(tree_pos)
    } else {
        None
    }
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
pub enum CommitOrText {
    Commit(Commit),
    InfoText(InfoText),
}

impl CommitOrText {
    fn load_all(global_args: &GlobalArgs, revset: &str) -> Result<Vec<Self>> {
        let output = JjCommand::log(revset, global_args.clone()).run()?;
        let mut lines = output.trim().lines();
        let re = Regex::new(r"^.+([k-z]{8}(?:/\d+)?)\s+.*\s+([a-f0-9]{8}).*$")?;

        let mut commits_or_texts = Vec::new();
        loop {
            let line1 = match lines.next() {
                None => break,
                Some(line) => line,
            };

            if re.captures(&strip_ansi(line1)).is_none() {
                commits_or_texts.push(Self::InfoText(InfoText::new(line1.to_string())));
                continue;
            };

            let line2 = lines.next().unwrap_or_default();
            commits_or_texts.push(Self::Commit(Commit::new(format!("{line1}\n{line2}"))?));
        }

        Ok(commits_or_texts)
    }

    fn flatten(
        &mut self,
        tree_pos: TreePosition,
        log_list: &mut Vec<Text<'static>>,
        log_list_tree_positions: &mut Vec<TreePosition>,
    ) -> Result<()> {
        match self {
            CommitOrText::Commit(commit) => {
                commit.flatten(tree_pos, log_list, log_list_tree_positions)
            }
            CommitOrText::InfoText(info_text) => {
                info_text.flatten(tree_pos, log_list, log_list_tree_positions)
            }
        }
    }

    pub fn flat_log_idx(&self) -> usize {
        match self {
            CommitOrText::Commit(commit) => commit.flat_log_idx(),
            CommitOrText::InfoText(info_text) => info_text.flat_log_idx,
        }
    }
}

#[derive(Debug)]
pub struct Commit {
    pub change_id: String,
    _commit_id: String,
    pub current_working_copy: bool,
    has_conflict: bool,
    _empty: bool,
    pub description_first_line: Option<String>,
    pub email: String,
    pub timestamp: String,
    pub bookmarks: Vec<String>,
    symbol: String,
    line1_graph_chars: String,
    line1_graph_chars_part2: String,
    line2_graph_chars: String,
    pretty_line1: String,
    pretty_line2: String,
    graph_indent: String,
    unfolded: bool,
    loaded: bool,
    file_diffs: Vec<FileDiff>,
    pub flat_log_idx: usize,
}

impl Commit {
    fn new(pretty_string: String) -> Result<Self> {
        let clean_string = strip_ansi(&pretty_string);
        let re_lines = Regex::new(r"^[ │]*\S+[ │]*(.*)\n[ │├┤┬┴╭╮╯╰─┼]*(.*)")?;

        if clean_string.contains("root()") {
            return Self::new_root(clean_string, pretty_string, &re_lines);
        }

        let re_fields = Regex::new(
            r"^([ │]*)(.)([ │]*)  ([k-z]{8,}(?:/\d+)?)\s+(\S+)\s+(\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2})\s*(.*?)\s+([a-f0-9]{8,})\s*(\S*)\s*\n([ │├┤┬┴╭╮╯╰─┼]*)(\(empty\))?\s*(.*)",
        )?;

        let captures = re_fields
            .captures(&clean_string)
            .ok_or_else(|| anyhow!("Cannot parse commit fields: {:?}", clean_string))?;
        let line1_graph_chars: String = captures[1].into();
        let symbol: String = captures[2].into();
        let line1_graph_chars_part2: String = captures[3].into();
        let change_id: String = captures[4].into();
        let email: String = captures[5].into();
        let timestamp: String = captures[6].into();
        let bookmarks_str: &str = &captures[7];
        let commit_id: String = captures[8].into();
        let conflict_status: &str = &captures[9];
        let line2_graph_chars: String = captures[10].into();
        let empty = captures.get(11).is_some();
        let description_string: &str = &captures[12];

        let bookmarks: Vec<String> = bookmarks_str
            .split_whitespace()
            .map(|s| s.to_string())
            .collect();

        let current_working_copy = symbol == "@";
        let has_conflict = conflict_status == "(conflict)";
        let description_first_line = if description_string == "(no description set)" {
            None
        } else {
            Some(description_string.to_string())
        };

        let captures = re_lines
            .captures(&pretty_string)
            .ok_or_else(|| anyhow!("Cannot parse commit lines: {:?}", pretty_string))?;
        let pretty_line1: String = captures[1].into();
        let pretty_line2: String = captures[2].into();

        let mut graph_indent: String = line2_graph_chars
            .chars()
            .map(|c| match c {
                '│' | '├' | '┤' | '┬' | '╭' | '╮' | '┼' => '│',
                _ => ' ',
            })
            .collect();
        graph_indent.pop();

        Ok(Commit {
            change_id,
            _commit_id: commit_id,
            current_working_copy,
            has_conflict,
            _empty: empty,
            description_first_line,
            email,
            timestamp,
            bookmarks,
            symbol,
            line1_graph_chars,
            line1_graph_chars_part2,
            line2_graph_chars,
            pretty_line1,
            pretty_line2,
            graph_indent,
            unfolded: false,
            loaded: false,
            file_diffs: Vec::new(),
            flat_log_idx: 0,
        })
    }

    fn new_root(clean_string: String, pretty_string: String, re_lines: &Regex) -> Result<Self> {
        let re_root = Regex::new(
            r"^([ │]*)(.)([ │]*)  ([k-z]{8,}(?:/\d+)?)\s+.*\s+([a-f0-9]{8,})\s*\n([ │├┤┬┴╭╮╯╰─┼]*)(.*)",
        )?;

        let captures = re_root
            .captures(&clean_string)
            .ok_or_else(|| anyhow!("Cannot parse root commit: {:?}", clean_string))?;
        let line1_graph_chars: String = captures[1].into();
        let symbol: String = captures[2].into();
        let line1_graph_chars_part2: String = captures[3].into();
        let change_id: String = captures[4].into();
        let commit_id: String = captures[5].into();
        let line2_graph_chars: String = captures[6].into();

        let captures = re_lines
            .captures(&pretty_string)
            .ok_or_else(|| anyhow!("Cannot parse root commit lines: {:?}", pretty_string))?;
        let pretty_line1: String = captures[1].into();
        let pretty_line2: String = captures[2].into();

        let mut graph_indent: String = line2_graph_chars
            .chars()
            .map(|c| match c {
                '│' | '├' | '┤' | '┬' | '╭' | '╮' | '┼' => '│',
                _ => ' ',
            })
            .collect();
        graph_indent.pop();

        Ok(Commit {
            change_id,
            _commit_id: commit_id,
            current_working_copy: false,
            has_conflict: false,
            _empty: false,
            description_first_line: None,
            email: String::new(),
            timestamp: String::new(),
            bookmarks: Vec::new(),
            symbol,
            line1_graph_chars,
            line1_graph_chars_part2,
            line2_graph_chars,
            pretty_line1,
            pretty_line2,
            graph_indent,
            unfolded: false,
            loaded: false,
            file_diffs: Vec::new(),
            flat_log_idx: 0,
        })
    }
}

impl LogTreeNode for Commit {
    fn render(&self) -> Result<Text<'static>> {
        let mut line1 = Line::from(vec![
            Span::raw(self.line1_graph_chars.clone()),
            Span::styled(
                self.symbol.clone(),
                if self.has_conflict {
                    Style::default().fg(Color::Red)
                } else if self.current_working_copy {
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::LightCyan)
                },
            ),
            Span::raw(self.line1_graph_chars_part2.clone()),
            Span::raw(" "),
            fold_symbol(self.unfolded),
            Span::raw(" "),
        ]);
        line1.extend(self.pretty_line1.into_text()?.lines[0].spans.clone());
        let mut lines = vec![line1];
        if !self.pretty_line2.is_empty() {
            let mut line2 = Line::from(vec![
                Span::raw(self.line2_graph_chars.clone()),
                Span::raw(" "),
            ]);
            line2.extend(self.pretty_line2.into_text()?.lines[0].spans.clone());
            lines.push(line2);
        };
        Ok(Text::from(lines))
    }

    fn flatten(
        &mut self,
        tree_pos: TreePosition,
        log_list: &mut Vec<Text<'static>>,
        log_list_tree_positions: &mut Vec<TreePosition>,
    ) -> Result<()> {
        self.flat_log_idx = log_list.len();
        log_list.push(self.render()?);
        log_list_tree_positions.push(tree_pos.clone());

        if !self.unfolded {
            return Ok(());
        }

        for (file_diff_idx, file_diff) in self.file_diffs.iter_mut().enumerate() {
            let mut new_pos = tree_pos.clone();
            new_pos.push(file_diff_idx);
            file_diff.flatten(new_pos, log_list, log_list_tree_positions)?;
        }

        Ok(())
    }

    fn flat_log_idx(&self) -> usize {
        self.flat_log_idx
    }

    fn children(&self) -> Vec<&dyn LogTreeNode> {
        self.file_diffs
            .iter()
            .map(|fd| fd as &dyn LogTreeNode)
            .collect()
    }

    fn toggle_fold(&mut self, global_args: &GlobalArgs) -> Result<()> {
        self.unfolded = !self.unfolded;
        if !self.unfolded {
            return Ok(());
        }

        if !self.loaded {
            let file_diffs = FileDiff::load_all(global_args, &self.change_id, &self.graph_indent)?;
            self.file_diffs = file_diffs;
            self.loaded = true;
        }

        Ok(())
    }
}

#[derive(Debug)]
pub struct InfoText {
    pretty_string: String,
    flat_log_idx: usize,
}

impl InfoText {
    fn new(pretty_string: String) -> Self {
        Self {
            pretty_string,
            flat_log_idx: 0,
        }
    }
}

impl LogTreeNode for InfoText {
    fn render(&self) -> Result<Text<'static>> {
        Ok(self.pretty_string.into_text()?)
    }

    fn flatten(
        &mut self,
        tree_pos: TreePosition,
        log_list: &mut Vec<Text<'static>>,
        log_list_tree_positions: &mut Vec<TreePosition>,
    ) -> Result<()> {
        self.flat_log_idx = log_list.len();
        log_list.push(self.render()?);
        log_list_tree_positions.push(tree_pos.clone());
        Ok(())
    }

    fn flat_log_idx(&self) -> usize {
        self.flat_log_idx
    }

    fn children(&self) -> Vec<&dyn LogTreeNode> {
        Vec::new()
    }

    fn toggle_fold(&mut self, _global_args: &GlobalArgs) -> Result<()> {
        Ok(())
    }
}

#[derive(Debug)]
pub struct FileDiff {
    change_id: String,
    pub path: String,
    description: String,
    status: FileDiffStatus,
    graph_indent: String,
    unfolded: bool,
    loaded: bool,
    diff_hunks: Vec<DiffHunk>,
    flat_log_idx: usize,
}

impl FileDiff {
    fn new(change_id: String, pretty_string: String, graph_indent: String) -> Result<Self> {
        let clean_string = strip_ansi(&pretty_string);
        let re = Regex::new(r"^([MADRC])\s+(.+)$").unwrap();

        let captures = re
            .captures(&clean_string)
            .ok_or_else(|| anyhow!("Cannot parse file diff string: {clean_string}"))?;
        let status = captures
            .get(1)
            .ok_or_else(|| anyhow!("Cannot parse file diff status"))?
            .as_str()
            .parse::<FileDiffStatus>()?;
        let description: String = captures
            .get(2)
            .ok_or_else(|| anyhow!("Cannot parse file diff path"))?
            .as_str()
            .into();

        let path = match status {
            FileDiffStatus::Renamed | FileDiffStatus::Copied => {
                let rename_regex = Regex::new(r"^(.*)\{(.+?)\s*=>\s*(.+?)\}(.*)$").unwrap();
                let captures = rename_regex.captures(&description).ok_or_else(|| {
                    anyhow!("Cannot parse file diff rename/copied paths: {description}")
                })?;
                let path_prefix = captures
                    .get(1)
                    .ok_or_else(|| anyhow!("Cannot parse file diff rename/copied path prefix"))?
                    .as_str();
                let path_new_end = captures
                    .get(3)
                    .ok_or_else(|| anyhow!("Cannot parse file diff rename/copied path new end"))?
                    .as_str();
                let path_suffix = captures
                    .get(4)
                    .ok_or_else(|| anyhow!("Cannot parse file diff rename/copied path suffix"))?
                    .as_str();

                format!("{path_prefix}{path_new_end}{path_suffix}")
            }
            _ => description.clone(),
        };

        Ok(Self {
            change_id,
            path,
            description,
            status,
            graph_indent,
            unfolded: false,
            loaded: false,
            diff_hunks: Vec::new(),
            flat_log_idx: 0,
        })
    }

    fn load_all(
        global_args: &GlobalArgs,
        change_id: &str,
        graph_indent: &str,
    ) -> Result<Vec<Self>> {
        let output = JjCommand::diff_summary(change_id, global_args.clone()).run()?;
        let lines: Vec<&str> = output.trim().lines().collect();

        let mut file_diffs = Vec::new();
        for line in lines {
            file_diffs.push(Self::new(
                change_id.to_string(),
                line.to_string(),
                graph_indent.to_string(),
            )?);
        }

        Ok(file_diffs)
    }
}

impl LogTreeNode for FileDiff {
    fn render(&self) -> Result<Text<'static>> {
        let line = Line::from(vec![
            Span::raw(self.graph_indent.clone()),
            fold_symbol(self.unfolded),
            Span::raw(" "),
            Span::styled(
                format!("{}  {}", self.status, self.description),
                Style::default().fg(Color::LightBlue),
            ),
        ]);
        Ok(Text::from(line))
    }

    fn flatten(
        &mut self,
        tree_pos: TreePosition,
        log_list: &mut Vec<Text<'static>>,
        log_list_tree_positions: &mut Vec<TreePosition>,
    ) -> Result<()> {
        self.flat_log_idx = log_list.len();
        log_list.push(self.render()?);
        log_list_tree_positions.push(tree_pos.clone());

        if !self.unfolded {
            return Ok(());
        }

        for (diff_hunk_idx, diff_hunk) in self.diff_hunks.iter_mut().enumerate() {
            let mut new_pos = tree_pos.clone();
            new_pos.push(diff_hunk_idx);
            diff_hunk.flatten(new_pos, log_list, log_list_tree_positions)?;
        }

        Ok(())
    }

    fn flat_log_idx(&self) -> usize {
        self.flat_log_idx
    }

    fn children(&self) -> Vec<&dyn LogTreeNode> {
        self.diff_hunks
            .iter()
            .map(|dh| dh as &dyn LogTreeNode)
            .collect()
    }

    fn toggle_fold(&mut self, global_args: &GlobalArgs) -> Result<()> {
        self.unfolded = !self.unfolded;

        if !self.loaded {
            let diff_hunks =
                DiffHunk::load_all(global_args, &self.change_id, &self.path, &self.graph_indent)?;
            self.diff_hunks = diff_hunks;
            self.loaded = true;
        }

        Ok(())
    }
}

#[derive(Debug)]
enum FileDiffStatus {
    Modified,
    Added,
    Deleted,
    Renamed,
    Copied,
}

impl std::str::FromStr for FileDiffStatus {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "M" => Ok(FileDiffStatus::Modified),
            "A" => Ok(FileDiffStatus::Added),
            "D" => Ok(FileDiffStatus::Deleted),
            "R" => Ok(FileDiffStatus::Renamed),
            "C" => Ok(FileDiffStatus::Copied),
            _ => Err(anyhow!("Unknown file diff status: {}", s)),
        }
    }
}

impl fmt::Display for FileDiffStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let word = match self {
            FileDiffStatus::Modified => "modified",
            FileDiffStatus::Added => "new file",
            FileDiffStatus::Deleted => "deleted ",
            FileDiffStatus::Renamed => "renamed ",
            FileDiffStatus::Copied => "copied  ",
        };
        write!(f, "{word}")
    }
}

#[derive(Debug)]
struct DiffHunk {
    graph_indent: String,
    unfolded: bool,
    diff_hunk_lines: Vec<DiffHunkLine>,
    red_start: u32,
    red_end: u32,
    green_start: u32,
    green_end: u32,
    flat_log_idx: usize,
}

enum SearchDirection {
    Down,
    Up,
}

impl DiffHunk {
    fn new(graph_indent: String, diff_hunk_lines: Vec<DiffHunkLine>) -> Result<Self> {
        let (red_start, green_start) =
            Self::find_line_nums(&diff_hunk_lines, SearchDirection::Down)?;
        let (red_end, green_end) = Self::find_line_nums(&diff_hunk_lines, SearchDirection::Up)?;

        // Align line nums
        let max_line_num = red_end.max(green_end);
        let line_num_chars_len = max_line_num.checked_ilog10().unwrap_or(0) as usize;
        let mut diff_hunk_lines = diff_hunk_lines;
        for line in diff_hunk_lines.iter_mut() {
            line.pretty_string = line.pretty_string.replacen(
                &" ".repeat(3_usize.saturating_sub(line_num_chars_len)),
                "",
                1,
            );
        }

        Ok(Self {
            graph_indent,
            unfolded: true,
            diff_hunk_lines,
            red_start,
            red_end,
            green_start,
            green_end,
            flat_log_idx: 0,
        })
    }

    fn find_line_nums(
        diff_hunk_lines: &[DiffHunkLine],
        direction: SearchDirection,
    ) -> Result<(u32, u32)> {
        let line_nums_regex = Regex::new(r"^\s*(\d+)?\s+(\d+)?:").unwrap();
        let mut red: Option<String> = None;
        let mut green: Option<String> = None;

        let hunk_lines: Vec<&DiffHunkLine> = match direction {
            SearchDirection::Down => diff_hunk_lines.iter().collect(),
            SearchDirection::Up => diff_hunk_lines.iter().rev().collect(),
        };
        for line in hunk_lines.iter().map(|l| strip_ansi(&l.pretty_string)) {
            if matches!(line.trim(), "~" | "(binary)" | "(empty)") {
                continue;
            }
            let captures = line_nums_regex
                .captures(&line)
                .ok_or_else(|| anyhow!("Cannot parse diff hunk line: {line:?}"))?;
            if red.is_none()
                && let Some(num_match) = captures.get(1)
                && !num_match.is_empty()
            {
                red = Some(num_match.as_str().to_string())
            }
            if green.is_none()
                && let Some(num_match) = captures.get(2)
                && !num_match.is_empty()
            {
                green = Some(num_match.as_str().to_string())
            }

            if red.is_some() && green.is_some() {
                break;
            }
        }

        if red.is_none() {
            red = Some("0".to_string());
        }
        if green.is_none() {
            green = Some("0".to_string());
        }

        Ok((red.unwrap().parse()?, green.unwrap().parse()?))
    }

    fn load_all(
        global_args: &GlobalArgs,
        change_id: &str,
        file: &str,
        graph_indent: &str,
    ) -> Result<Vec<Self>> {
        let output = JjCommand::diff_file(change_id, file, global_args.clone()).run()?;
        let output_lines: Vec<&str> = output.trim().lines().skip(1).collect();

        let separator_regex = Regex::new(r"^\s*\.\.\.\s*$")?;
        let mut diff_hunks: Vec<DiffHunk> = Vec::new();
        let mut diff_hunk_lines = Vec::new();

        let mut push_diff_hunk = |diff_hunk_lines: Vec<DiffHunkLine>| -> Result<()> {
            if !diff_hunk_lines.is_empty() {
                diff_hunks.push(Self::new(graph_indent.to_string(), diff_hunk_lines)?);
            }
            Ok(())
        };

        for line in output_lines {
            let clean_line = strip_ansi(line);

            if separator_regex.is_match(&clean_line) {
                push_diff_hunk(diff_hunk_lines)?;
                diff_hunk_lines = Vec::new();
            } else {
                diff_hunk_lines.push(DiffHunkLine::new(
                    line.to_string(),
                    graph_indent.to_string(),
                ));
            }
        }

        push_diff_hunk(diff_hunk_lines)?;

        // Visual divider between hunk diff and next item in log list
        if !diff_hunks.is_empty() {
            diff_hunks
                .last_mut()
                .unwrap()
                .diff_hunk_lines
                .push(DiffHunkLine::new(
                    "\x1b[35m~\x1b[0m".to_string(),
                    graph_indent.to_string(),
                ));
        }

        Ok(diff_hunks)
    }
}

impl LogTreeNode for DiffHunk {
    fn render(&self) -> Result<Text<'static>> {
        let red_num_lines = if self.red_end == 0 {
            0
        } else {
            self.red_end - self.red_start + 1
        };
        let green_num_lines = if self.green_end == 0 {
            0
        } else {
            self.green_end - self.green_start + 1
        };

        let line = Line::from(vec![
            Span::raw(self.graph_indent.clone()),
            fold_symbol(self.unfolded),
            Span::raw(" "),
            Span::styled(
                format!(
                    "@@ -{},{} +{},{} @@",
                    self.red_start, red_num_lines, self.green_start, green_num_lines,
                ),
                Style::default().fg(Color::Magenta),
            ),
        ]);
        Ok(Text::from(line))
    }

    fn flatten(
        &mut self,
        tree_pos: TreePosition,
        log_list: &mut Vec<Text<'static>>,
        log_list_tree_positions: &mut Vec<TreePosition>,
    ) -> Result<()> {
        self.flat_log_idx = log_list.len();
        log_list.push(self.render()?);
        log_list_tree_positions.push(tree_pos.clone());

        if !self.unfolded {
            return Ok(());
        }

        for (diff_hunk_line_idx, diff_hunk_line) in self.diff_hunk_lines.iter_mut().enumerate() {
            let mut new_pos = tree_pos.clone();
            new_pos.push(diff_hunk_line_idx);
            diff_hunk_line.flatten(new_pos, log_list, log_list_tree_positions)?;
        }

        Ok(())
    }

    fn flat_log_idx(&self) -> usize {
        self.flat_log_idx
    }

    fn children(&self) -> Vec<&dyn LogTreeNode> {
        self.diff_hunk_lines
            .iter()
            .map(|hl| hl as &dyn LogTreeNode)
            .collect()
    }

    fn toggle_fold(&mut self, _global_args: &GlobalArgs) -> Result<()> {
        self.unfolded = !self.unfolded;
        Ok(())
    }
}

#[derive(Debug)]
struct DiffHunkLine {
    pretty_string: String,
    graph_indent: String,
    flat_log_idx: usize,
}

impl DiffHunkLine {
    fn new(pretty_string: String, graph_indent: String) -> Self {
        Self {
            pretty_string,
            graph_indent,
            flat_log_idx: 0,
        }
    }
}

impl LogTreeNode for DiffHunkLine {
    fn render(&self) -> Result<Text<'static>> {
        let clean_string = strip_ansi(&self.pretty_string);
        let mut line = Line::from(vec![Span::raw(self.graph_indent.clone()), Span::raw("  ")]);

        for span in self.pretty_string.into_text()?.lines[0].spans.clone() {
            let span = if clean_string.starts_with("+") || clean_string.starts_with("-") {
                let style = span.style.bold();
                span.style(style)
            } else {
                span
            };
            line.spans.push(span);
        }

        Ok(Text::from(line))
    }

    fn flatten(
        &mut self,
        tree_pos: TreePosition,
        log_list: &mut Vec<Text<'static>>,
        log_list_tree_positions: &mut Vec<TreePosition>,
    ) -> Result<()> {
        self.flat_log_idx = log_list.len();
        log_list.push(self.render()?);
        log_list_tree_positions.push(tree_pos);
        Ok(())
    }

    fn flat_log_idx(&self) -> usize {
        self.flat_log_idx
    }

    fn children(&self) -> Vec<&dyn LogTreeNode> {
        Vec::new()
    }

    fn toggle_fold(&mut self, _global_args: &GlobalArgs) -> Result<()> {
        Ok(())
    }
}

fn fold_symbol(unfolded: bool) -> Span<'static> {
    let symbol = if unfolded { "▾" } else { "▸" };
    Span::styled(symbol, Style::default().fg(Color::DarkGray))
}

fn strip_ansi(pretty_str: &str) -> String {
    let ansi_regex = Regex::new(r"\x1b\[[0-9;]*m").unwrap();
    ansi_regex.replace_all(pretty_str, "").to_string()
}
