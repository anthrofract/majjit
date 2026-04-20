use crate::{
    command_tree::{CommandTree, display_unbound_error_lines},
    log_tree::{DIFF_HUNK_LINE_IDX, JjLog, LogTreeNode, TreePosition, get_parent_tree_position},
    shell_out::{JjCommand, JjCommandError, open_file_in_editor},
    terminal::Term,
    update::{
        AbandonMode, AbsorbMode, BookmarkMoveMode, DuplicateDestination, DuplicateDestinationType,
        GitFetchMode, GitPushMode, InterdiffMode, Message, MetaeditAction, NewMode,
        NextPrevDirection, NextPrevMode, ParallelizeSource, RebaseDestination,
        RebaseDestinationType, RebaseSourceType, RestoreMode, RevertDestination,
        RevertDestinationType, RevertRevision, SetRevsetMode, SignAction, SimplifyParentsMode,
        SquashMode, ViewMode,
    },
};
use ansi_to_tui::IntoText;
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Text},
    widgets::ListState,
};
use ratatui_textarea::{CursorMove, TextArea};

pub const DEFAULT_REVSET: &str =
    "present(@) | ancestors(immutable_heads().., 32) | remote_bookmarks() | root()";

const LOG_LIST_SCROLL_PADDING: usize = 5;

#[derive(Default, Debug, PartialEq, Eq)]
pub enum State {
    #[default]
    Running,
    EnteringText,
    Quit,
}

#[derive(Debug)]
pub struct TextInputSession {
    pub prompt: String,
    pub textarea: TextArea<'static>,
    pub action: TextInputAction,
    pub fuzzy: Option<FuzzyFinderState>,
}

#[derive(Debug)]
pub struct FuzzyFinderState {
    pub candidates: Vec<FuzzyCandidate>,
    pub filtered: Vec<FilteredCandidate>,
    pub selected: usize,
}

#[derive(Debug)]
pub struct FuzzyCandidate {
    pub display: String,
    pub target: Option<String>,
}

impl FuzzyCandidate {
    pub fn from_display(display: String) -> Self {
        Self {
            display,
            target: None,
        }
    }
}

#[derive(Debug)]
pub struct FilteredCandidate {
    pub candidate_index: usize,
    pub score: i64,
    pub match_positions: Vec<usize>,
}

#[derive(Debug, Clone)]
pub enum TextInputAction {
    SetRevset,
    Describe,
    BookmarkCreate,
    BookmarkDelete,
    BookmarkForget {
        include_remotes: bool,
    },
    BookmarkRenameFrom,
    BookmarkRenameTo {
        old_name: String,
    },
    BookmarkSet,
    BookmarkTrack,
    BookmarkUntrack,
    EditTarget,
    FileTrack,
    GitFetchBranch,
    GitFetchRemote,
    GitPushNamed {
        change_id: String,
    },
    GitPushBookmark,
    MetaeditAuthor {
        change_id: String,
    },
    MetaeditAuthorTimestamp {
        change_id: String,
    },
    NewAtTarget,
    NextPrevOffset {
        direction: NextPrevDirection,
        mode: NextPrevMode,
    },
    ParallelizeRevset,
    RebaseTarget {
        source_type: RebaseSourceType,
        destination_type: RebaseDestinationType,
    },
    SelectInRevset,
    WorkspaceAddPathOnly,
    WorkspaceAddNamePrompt,
    WorkspaceAddPathPrompt {
        name: String,
    },
    WorkspaceForget,
    WorkspaceList,
    WorkspaceRename,
}

#[derive(Debug, Clone)]
pub struct GlobalArgs {
    pub repository: String,
    pub ignore_immutable: bool,
}

#[derive(Debug)]
pub struct Model {
    pub global_args: GlobalArgs,
    pub display_repository: String,
    pub revset: String,
    pub state: State,
    pub command_tree: CommandTree,
    command_keys: Vec<KeyCode>,
    queued_jj_commands: Vec<JjCommand>,
    accumulated_command_output: Vec<Line<'static>>,
    saved_change_id: Option<String>,
    saved_file_path: Option<String>,
    saved_tree_position: Option<TreePosition>,
    jj_log: JjLog,
    pub log_list: Vec<Text<'static>>,
    pub log_list_state: ListState,
    log_list_tree_positions: Vec<TreePosition>,
    pub log_list_layout: Rect,
    pub log_list_scroll_padding: usize,
    pub fuzzy_viewport_height: usize,
    pub info_list: Option<Text<'static>>,
    pub text_input: Option<TextInputSession>,
}

#[derive(Debug)]
enum ScrollDirection {
    Up,
    Down,
}

impl Model {
    pub fn new(repository: String, revset: String) -> Result<Self> {
        let mut model = Self {
            state: State::default(),
            command_tree: CommandTree::new(),
            command_keys: Vec::new(),
            queued_jj_commands: Vec::new(),
            accumulated_command_output: Vec::new(),
            saved_tree_position: None,
            saved_change_id: None,
            saved_file_path: None,
            jj_log: JjLog::new()?,
            log_list: Vec::new(),
            log_list_state: ListState::default(),
            log_list_tree_positions: Vec::new(),
            log_list_layout: Rect::ZERO,
            log_list_scroll_padding: LOG_LIST_SCROLL_PADDING,
            fuzzy_viewport_height: 0,
            info_list: None,
            text_input: None,
            display_repository: format_repository_for_display(&repository),
            global_args: GlobalArgs {
                repository,
                ignore_immutable: false,
            },
            revset,
        };

        model.sync()?;
        Ok(model)
    }

    pub fn quit(&mut self) {
        self.state = State::Quit;
    }

    fn reset_log_list_selection(&mut self) -> Result<()> {
        // Start with @ selected and unfolded
        let list_idx = match self.jj_log.get_current_commit() {
            None => 0,
            Some(commit) => commit.flat_log_idx(),
        };
        self.log_select(list_idx);
        self.toggle_current_fold()
    }

    pub fn sync(&mut self) -> Result<()> {
        self.jj_log.load_log_tree(&self.global_args, &self.revset)?;
        self.sync_log_list()?;
        self.reset_log_list_selection()?;
        Ok(())
    }

    fn sync_log_list(&mut self) -> Result<()> {
        (self.log_list, self.log_list_tree_positions) = self.jj_log.flatten_log()?;
        Ok(())
    }

    pub fn refresh(&mut self) -> Result<()> {
        // Add periods for visual feedback on repeated refreshes
        let periods = self
            .info_list
            .as_ref()
            .map(|t| t.to_string())
            .filter(|s| s.starts_with("Refreshed"))
            .map_or(0, |s| s.matches('.').count() + 3);
        self.clear();
        self.sync()?;
        self.info_list = Some(format!("Refreshed{}", ".".repeat(periods)).into());
        Ok(())
    }

    pub fn toggle_ignore_immutable(&mut self) {
        self.global_args.ignore_immutable = !self.global_args.ignore_immutable;
    }

    fn log_offset(&self) -> usize {
        self.log_list_state.offset()
    }

    fn log_selected(&self) -> usize {
        self.log_list_state.selected().unwrap()
    }

    fn log_select(&mut self, idx: usize) {
        self.log_list_state.select(Some(idx));
    }

    fn get_selected_tree_position(&self) -> TreePosition {
        self.log_list_tree_positions[self.log_selected()].clone()
    }

    fn get_selected_change_id(&self) -> Option<&str> {
        let tree_pos = self.get_selected_tree_position();
        self.get_change_id(tree_pos)
    }

    fn get_saved_change_id(&self) -> Option<&str> {
        self.saved_change_id.as_deref()
    }

    fn get_change_id(&self, tree_pos: TreePosition) -> Option<&str> {
        match self.jj_log.get_tree_commit(&tree_pos) {
            None => None,
            Some(commit) => Some(&commit.change_id),
        }
    }

    fn get_selected_commit_id(&self) -> Option<&str> {
        let tree_pos = self.get_selected_tree_position();
        match self.jj_log.get_tree_commit(&tree_pos) {
            None => None,
            Some(commit) => Some(&commit.commit_id),
        }
    }

    fn get_selected_workspaces(&self) -> Vec<String> {
        let tree_pos = self.get_selected_tree_position();
        let Some(commit) = self.jj_log.get_tree_commit(&tree_pos) else {
            return Vec::new();
        };
        commit
            .workspaces
            .iter()
            .map(|w| w.strip_suffix('@').unwrap_or(w).to_string())
            .collect()
    }

    fn get_selected_file_path(&self) -> Option<&str> {
        let tree_pos = self.get_selected_tree_position();
        self.get_file_path(tree_pos)
    }

    fn get_saved_file_path(&self) -> Option<&str> {
        self.saved_file_path.as_deref()
    }

    fn get_file_path(&self, tree_pos: TreePosition) -> Option<&str> {
        match self.jj_log.get_tree_file_diff(&tree_pos) {
            None => None,
            Some(file_diff) => Some(&file_diff.path),
        }
    }

    pub fn get_saved_selection_flat_log_idxs(&self) -> (Option<usize>, Option<usize>) {
        let Some(saved_tree_position) = self.saved_tree_position.as_ref() else {
            return (None, None);
        };

        let commit_idx = self
            .jj_log
            .get_tree_commit(saved_tree_position)
            .map(|commit| commit.flat_log_idx());
        let file_diff_idx = self
            .jj_log
            .get_tree_file_diff(saved_tree_position)
            .map(|file_diff| file_diff.flat_log_idx());

        (commit_idx, file_diff_idx)
    }

    fn is_selected_working_copy(&self) -> bool {
        let tree_pos = self.get_selected_tree_position();
        match self.jj_log.get_tree_commit(&tree_pos) {
            None => false,
            Some(commit) => commit.current_working_copy,
        }
    }

    fn get_bookmark_names(&self) -> Result<Vec<String>> {
        let cmd = JjCommand::jj_bookmark_list_all_names(self.global_args.clone());
        let output = cmd.run().map_err(|e| anyhow::anyhow!("{}", e))?;
        let mut names: Vec<String> = output
            .lines()
            .map(|line| line.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        names.sort();
        names.dedup();
        Ok(names)
    }

    fn get_workspace_names(&self) -> Result<Vec<String>> {
        let cmd = JjCommand::jj_workspace_list_names(self.global_args.clone());
        let output = cmd.run().map_err(|e| anyhow::anyhow!("{}", e))?;
        let mut names: Vec<String> = output
            .lines()
            .map(|line| line.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        names.sort();
        names.dedup();
        Ok(names)
    }

    fn get_current_workspace_name(&self) -> Result<String> {
        let cmd = JjCommand::jj_workspace_list_current_name(self.global_args.clone());
        let output = cmd.run().map_err(|e| anyhow::anyhow!("{}", e))?;
        Ok(output
            .lines()
            .map(|line| line.trim().to_string())
            .find(|s| !s.is_empty())
            .unwrap_or_default())
    }

    fn get_tracked_remote_bookmarks(&self) -> Result<Vec<String>> {
        let cmd = JjCommand::jj_bookmark_list_tracked_remote(self.global_args.clone());
        let output = cmd.run().map_err(|e| anyhow::anyhow!("{}", e))?;
        let mut names: Vec<String> = output
            .lines()
            .map(|line| line.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        names.sort();
        names.dedup();
        Ok(names)
    }

    fn get_untracked_remote_bookmarks(&self) -> Result<Vec<String>> {
        let cmd = JjCommand::jj_bookmark_list_untracked_remote(self.global_args.clone());
        let output = cmd.run().map_err(|e| anyhow::anyhow!("{}", e))?;
        let mut names: Vec<String> = output
            .lines()
            .map(|line| line.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        names.sort();
        names.dedup();
        Ok(names)
    }

    fn get_git_remote_names(&self) -> Result<Vec<String>> {
        let cmd = JjCommand::jj_git_remote_list(self.global_args.clone());
        let output = cmd.run().map_err(|e| anyhow::anyhow!("{}", e))?;
        let remotes = output
            .lines()
            .filter_map(|line| line.split_whitespace().next())
            .map(|s| s.to_string())
            .collect();
        Ok(remotes)
    }

    fn get_revision_targets(&self) -> Result<Vec<String>> {
        let cmd = JjCommand::jj_log_targets(&self.revset, self.global_args.clone());
        let output = cmd.run().map_err(|e| anyhow::anyhow!("{}", e))?;
        let mut targets: Vec<String> = output
            .lines()
            .map(|line| line.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        targets.sort();
        targets.dedup();
        Ok(targets)
    }

    fn get_file_list(&self) -> Result<Vec<String>> {
        let cmd = JjCommand::jj_file_list(self.global_args.clone());
        let output = cmd.run().map_err(|e| anyhow::anyhow!("{}", e))?;
        let names: Vec<String> = output
            .lines()
            .map(|line| line.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        Ok(names)
    }

    pub fn select_next_node(&mut self) {
        if self.log_list_state.selected().unwrap() < self.log_list.len() - 1 {
            self.log_list_state.select_next();
        }
    }

    pub fn select_prev_node(&mut self) {
        if self.log_list_state.selected().unwrap() > 0 {
            self.log_list_state.select_previous();
        }
    }

    pub fn select_current_working_copy(&mut self) {
        if let Some(commit) = self.jj_log.get_current_commit() {
            self.log_select(commit.flat_log_idx());
        }
    }

    fn log_revset_candidates(&self, with_log_idx_targets: bool) -> Vec<FuzzyCandidate> {
        let mut candidates: Vec<FuzzyCandidate> = Vec::new();

        for item in &self.jj_log.log_tree {
            let crate::log_tree::CommitOrText::Commit(commit) = item else {
                continue;
            };
            let target = if with_log_idx_targets {
                Some(commit.flat_log_idx().to_string())
            } else {
                None
            };

            candidates.push(FuzzyCandidate {
                display: commit.change_id.clone(),
                target: target.clone(),
            });
            candidates.push(FuzzyCandidate {
                display: commit.commit_id.clone(),
                target: target.clone(),
            });
            for bookmark in &commit.bookmarks {
                candidates.push(FuzzyCandidate {
                    display: bookmark.clone(),
                    target: target.clone(),
                });
            }
            for workspace in &commit.workspaces {
                candidates.push(FuzzyCandidate {
                    display: workspace.clone(),
                    target: target.clone(),
                });
            }
        }

        candidates
    }

    pub fn select_in_revset(&mut self) {
        let candidates = self.log_revset_candidates(true);
        self.start_fuzzy_input("Select", candidates, TextInputAction::SelectInRevset);
    }

    pub fn select_by_description(&mut self) {
        let mut candidates: Vec<FuzzyCandidate> = Vec::new();

        for item in &self.jj_log.log_tree {
            let crate::log_tree::CommitOrText::Commit(commit) = item else {
                continue;
            };
            let Some(description) = &commit.description_first_line else {
                continue;
            };
            candidates.push(FuzzyCandidate {
                display: description.clone(),
                target: Some(commit.flat_log_idx().to_string()),
            });
        }

        self.start_fuzzy_input("Select", candidates, TextInputAction::SelectInRevset);
    }

    pub fn select_by_bookmark(&mut self) {
        let mut candidates: Vec<FuzzyCandidate> = Vec::new();

        for item in &self.jj_log.log_tree {
            let crate::log_tree::CommitOrText::Commit(commit) = item else {
                continue;
            };
            let target = Some(commit.flat_log_idx().to_string());
            for bookmark in &commit.bookmarks {
                candidates.push(FuzzyCandidate {
                    display: bookmark.clone(),
                    target: target.clone(),
                });
            }
        }

        self.start_fuzzy_input("Select", candidates, TextInputAction::SelectInRevset);
    }

    pub fn select_parent_node(&mut self) -> Result<()> {
        let tree_pos = self.get_selected_tree_position();
        if let Some(parent_pos) = get_parent_tree_position(&tree_pos) {
            let parent_node_idx = self.jj_log.get_tree_node(&parent_pos)?.flat_log_idx();
            self.log_select(parent_node_idx);
        }
        Ok(())
    }

    pub fn select_current_next_sibling_node(&mut self) -> Result<()> {
        let tree_pos = self.get_selected_tree_position();
        self.select_next_sibling_node(tree_pos)
    }

    fn select_next_sibling_node(&mut self, tree_pos: TreePosition) -> Result<()> {
        let mut tree_pos = tree_pos;
        if tree_pos.len() == DIFF_HUNK_LINE_IDX + 1 {
            tree_pos = get_parent_tree_position(&tree_pos).unwrap();
        }
        let idx = tree_pos[tree_pos.len() - 1];

        match get_parent_tree_position(&tree_pos) {
            Some(parent_pos) => {
                let parent_node = self.jj_log.get_tree_node(&parent_pos)?;
                let children = parent_node.children();

                if idx == children.len() - 1 {
                    self.select_next_sibling_node(parent_pos)?;
                } else {
                    let sibling_idx = (idx + 1).min(children.len() - 1);
                    self.log_list_state
                        .select(Some(children[sibling_idx].flat_log_idx()));
                }
            }
            None => {
                let sibling_idx = (idx + 1).min(self.jj_log.log_tree.len() - 1);
                self.log_list_state
                    .select(Some(self.jj_log.log_tree[sibling_idx].flat_log_idx()));
            }
        };

        Ok(())
    }

    pub fn select_current_prev_sibling_node(&mut self) -> Result<()> {
        let tree_pos = self.get_selected_tree_position();
        self.select_prev_sibling_node(tree_pos)
    }

    fn select_prev_sibling_node(&mut self, tree_pos: TreePosition) -> Result<()> {
        if tree_pos.len() == DIFF_HUNK_LINE_IDX + 1 {
            let parent_pos = get_parent_tree_position(&tree_pos).unwrap();
            let parent_node_idx = self.jj_log.get_tree_node(&parent_pos)?.flat_log_idx();
            self.log_select(parent_node_idx);
            return Ok(());
        }
        let idx = tree_pos[tree_pos.len() - 1];

        match get_parent_tree_position(&tree_pos) {
            Some(parent_pos) => {
                let parent_node = self.jj_log.get_tree_node(&parent_pos)?;
                let children = parent_node.children();

                if idx == 0 {
                    let parent_node_idx = parent_node.flat_log_idx();
                    self.log_select(parent_node_idx);
                } else {
                    let sibling_idx = idx - 1;
                    self.log_list_state
                        .select(Some(children[sibling_idx].flat_log_idx()));
                }
            }
            None => {
                let sibling_idx = idx.saturating_sub(1);
                self.log_list_state
                    .select(Some(self.jj_log.log_tree[sibling_idx].flat_log_idx()));
            }
        };

        Ok(())
    }

    pub fn toggle_current_fold(&mut self) -> Result<()> {
        let tree_pos = self.get_selected_tree_position();
        let log_list_selected_idx = self.jj_log.toggle_fold(&self.global_args, &tree_pos)?;
        self.sync_log_list()?;
        self.log_select(log_list_selected_idx);
        Ok(())
    }

    pub fn clear(&mut self) {
        self.state = State::Running;
        self.text_input = None;
        self.info_list = None;
        self.saved_tree_position = None;
        self.saved_change_id = None;
        self.saved_file_path = None;
        self.command_keys.clear();
        self.queued_jj_commands.clear();
        self.accumulated_command_output.clear();
    }

    /// User cancelled an action (e.g., closed editor without entering input).
    /// The command key sequence is automatically cleared by `handle_command_key`
    /// when the action is triggered, so we don't need to clear it here.
    fn cancelled(&mut self) -> Result<()> {
        self.info_list = Some(Text::from("Cancelled"));
        Ok(())
    }

    /// The selected or saved change is invalid for this operation (e.g., no
    /// change selected, or the saved selection from a two-step command is missing).
    /// The command key sequence is automatically cleared by `handle_command_key`
    /// when the action is triggered, so we don't need to clear it here.
    fn invalid_selection(&mut self) -> Result<()> {
        self.info_list = Some(Text::from("Invalid selection"));
        Ok(())
    }

    fn display_error_lines(&mut self, err: &anyhow::Error) {
        self.info_list = Some(err.to_string().into_text().unwrap());
    }

    fn start_text_input(&mut self, prompt: &str, initial_text: &str, action: TextInputAction) {
        let mut textarea = TextArea::new(vec![initial_text.to_string()]);
        textarea.move_cursor(CursorMove::End);
        textarea.set_cursor_line_style(Style::default());

        self.info_list = None;
        self.state = State::EnteringText;
        self.text_input = Some(TextInputSession {
            prompt: prompt.to_string(),
            textarea,
            action,
            fuzzy: None,
        });
    }

    pub fn submit_text_input(&mut self) -> Result<Option<Message>> {
        self.state = State::Running;
        let Some(session) = self.text_input.take() else {
            return Ok(None);
        };

        let maybe_value = match &session.fuzzy {
            Some(fuzzy) => {
                if fuzzy.filtered.is_empty() {
                    self.cancelled()?;
                    None
                } else {
                    let selected = &fuzzy.filtered[fuzzy.selected];
                    let candidate = &fuzzy.candidates[selected.candidate_index];
                    Some(
                        candidate
                            .target
                            .clone()
                            .unwrap_or_else(|| candidate.display.clone()),
                    )
                }
            }
            None => {
                let value = session.textarea.lines()[0].trim().to_string();
                if value.is_empty() {
                    self.cancelled()?;
                    None
                } else {
                    Some(value)
                }
            }
        };

        if let Some(value) = maybe_value {
            self.apply_text_input(session.action, value)?;
        }
        Ok(None)
    }

    pub fn forward_text_input_key(&mut self, key: KeyEvent) {
        if let Some(session) = self.text_input.as_mut() {
            session.textarea.input(key);
        }
    }

    fn start_fuzzy_input(
        &mut self,
        prompt: &str,
        mut candidates: Vec<FuzzyCandidate>,
        action: TextInputAction,
    ) {
        if candidates.is_empty() {
            self.start_text_input(prompt, "", action);
            return;
        }

        candidates.sort_by(|a, b| b.display.cmp(&a.display));

        let mut textarea = TextArea::new(vec![String::new()]);
        textarea.move_cursor(CursorMove::End);
        textarea.set_cursor_line_style(Style::default());

        let filtered: Vec<FilteredCandidate> = candidates
            .iter()
            .enumerate()
            .map(|(idx, _)| FilteredCandidate {
                candidate_index: idx,
                score: 0,
                match_positions: Vec::new(),
            })
            .collect();
        let selected = filtered.len().saturating_sub(1);

        self.info_list = None;
        self.state = State::EnteringText;
        self.text_input = Some(TextInputSession {
            prompt: prompt.to_string(),
            textarea,
            action,
            fuzzy: Some(FuzzyFinderState {
                candidates,
                filtered,
                selected,
            }),
        });
    }

    pub fn update_fuzzy_filter(&mut self) {
        use fuzzy_matcher::FuzzyMatcher;
        use fuzzy_matcher::skim::SkimMatcherV2;

        let Some(session) = self.text_input.as_mut() else {
            return;
        };
        let Some(fuzzy) = session.fuzzy.as_mut() else {
            return;
        };

        let query = session.textarea.lines()[0].trim().to_string();
        if query.is_empty() {
            fuzzy.filtered = fuzzy
                .candidates
                .iter()
                .enumerate()
                .map(|(idx, _)| FilteredCandidate {
                    candidate_index: idx,
                    score: 0,
                    match_positions: Vec::new(),
                })
                .collect();
            fuzzy.selected = fuzzy.filtered.len().saturating_sub(1);
            return;
        }

        let matcher = SkimMatcherV2::default();
        let mut filtered: Vec<FilteredCandidate> = fuzzy
            .candidates
            .iter()
            .enumerate()
            .filter_map(|(idx, candidate)| {
                let (score, positions) = matcher.fuzzy_indices(&candidate.display, &query)?;
                Some(FilteredCandidate {
                    candidate_index: idx,
                    score,
                    match_positions: positions,
                })
            })
            .collect();

        filtered.sort_by_key(|f| f.score);

        fuzzy.filtered = filtered;
        if fuzzy.filtered.is_empty() {
            fuzzy.selected = 0;
        } else {
            fuzzy.selected = fuzzy.filtered.len() - 1;
        }
    }

    pub fn move_fuzzy_selection_up(&mut self) {
        let Some(session) = self.text_input.as_mut() else {
            return;
        };
        let Some(fuzzy) = session.fuzzy.as_mut() else {
            return;
        };
        if fuzzy.filtered.is_empty() {
            return;
        }
        if fuzzy.selected == 0 {
            fuzzy.selected = fuzzy.filtered.len() - 1;
        } else {
            fuzzy.selected -= 1;
        }
    }

    pub fn move_fuzzy_selection_down(&mut self) {
        let Some(session) = self.text_input.as_mut() else {
            return;
        };
        let Some(fuzzy) = session.fuzzy.as_mut() else {
            return;
        };
        if fuzzy.filtered.is_empty() {
            return;
        }
        if fuzzy.selected >= fuzzy.filtered.len() - 1 {
            fuzzy.selected = 0;
        } else {
            fuzzy.selected += 1;
        }
    }

    pub fn page_fuzzy_selection_up(&mut self) {
        let Some(session) = self.text_input.as_mut() else {
            return;
        };
        let Some(fuzzy) = session.fuzzy.as_mut() else {
            return;
        };
        if fuzzy.filtered.is_empty() {
            return;
        }
        let page = self.fuzzy_viewport_height.max(1);
        fuzzy.selected = fuzzy.selected.saturating_sub(page);
    }

    pub fn page_fuzzy_selection_down(&mut self) {
        let Some(session) = self.text_input.as_mut() else {
            return;
        };
        let Some(fuzzy) = session.fuzzy.as_mut() else {
            return;
        };
        if fuzzy.filtered.is_empty() {
            return;
        }
        let page = self.fuzzy_viewport_height.max(1);
        fuzzy.selected = (fuzzy.selected + page).min(fuzzy.filtered.len() - 1);
    }

    pub fn has_active_fuzzy(&self) -> bool {
        self.text_input
            .as_ref()
            .and_then(|s| s.fuzzy.as_ref())
            .is_some()
    }

    fn apply_set_revset_from_input(&mut self, new_revset: String) -> Result<()> {
        let old_revset = self.revset.clone();
        self.revset = new_revset;

        match self.sync() {
            Err(err) => {
                self.display_error_lines(&err);
                self.revset = old_revset;
            }
            Ok(()) => {
                self.info_list = Some(Text::from(format!("Revset set to '{}'", self.revset)));
            }
        }
        Ok(())
    }

    pub fn set_revset(&mut self, mode: SetRevsetMode) {
        match mode {
            SetRevsetMode::Custom => {
                let initial_text = self.revset.clone();
                self.start_text_input("Revset", &initial_text, TextInputAction::SetRevset);
            }
            SetRevsetMode::Default => {
                let _ = self.apply_set_revset_from_input(DEFAULT_REVSET.to_string());
            }
            SetRevsetMode::JjDefault => {
                match JjCommand::jj_config_get_revsets_log(&self.global_args.repository) {
                    Ok(revset) => {
                        let _ = self.apply_set_revset_from_input(revset);
                    }
                    Err(err) => {
                        self.display_error_lines(&anyhow::anyhow!("{}", err));
                    }
                }
            }
            SetRevsetMode::All => {
                let _ = self.apply_set_revset_from_input("all()".to_string());
            }
            SetRevsetMode::Mutable => {
                let _ = self.apply_set_revset_from_input("mutable()".to_string());
            }
            SetRevsetMode::Stack => {
                let _ = self.apply_set_revset_from_input("trunk() | (trunk()..@)::".to_string());
            }
            SetRevsetMode::Conflicts => {
                let _ = self.apply_set_revset_from_input("conflicts()".to_string());
            }
            SetRevsetMode::WorkingCopyAncestry => {
                let _ = self.apply_set_revset_from_input("::@".to_string());
            }
            SetRevsetMode::Mine => {
                let _ = self.apply_set_revset_from_input("mine()".to_string());
            }
            SetRevsetMode::Bookmarks => {
                let _ = self.apply_set_revset_from_input(
                    "bookmarks() | remote_bookmarks() | tags()".to_string(),
                );
            }
            SetRevsetMode::Recent => {
                let _ = self.apply_set_revset_from_input(
                    "committer_date(after:\"1 week ago\")".to_string(),
                );
            }
        }
    }

    pub fn start_describe_input(&mut self) -> Result<()> {
        if self.get_selected_change_id().is_none() {
            return self.invalid_selection();
        }
        let tree_pos = self.get_selected_tree_position();
        let initial_description = self
            .jj_log
            .get_tree_commit(&tree_pos)
            .and_then(|commit| commit.description_first_line.as_deref())
            .unwrap_or("")
            .to_string();
        self.start_text_input("Describe", &initial_description, TextInputAction::Describe);
        Ok(())
    }

    fn apply_describe_from_input(&mut self, message: String) -> Result<()> {
        let Some(change_id) = self.get_selected_change_id() else {
            return self.invalid_selection();
        };

        let cmd =
            JjCommand::jj_describe_with_message(change_id, &message, self.global_args.clone());
        self.queue_jj_command(cmd)
    }

    fn apply_text_input(&mut self, action: TextInputAction, value: String) -> Result<()> {
        match action {
            TextInputAction::SetRevset => self.apply_set_revset_from_input(value),
            TextInputAction::Describe => self.apply_describe_from_input(value),
            TextInputAction::BookmarkCreate => self.apply_bookmark_create_from_input(value),
            TextInputAction::BookmarkDelete => self.apply_bookmark_delete_from_input(value),
            TextInputAction::BookmarkForget { include_remotes } => {
                self.apply_bookmark_forget_from_input(value, include_remotes)
            }
            TextInputAction::BookmarkRenameFrom => {
                self.start_text_input(
                    "Bookmark to",
                    "",
                    TextInputAction::BookmarkRenameTo { old_name: value },
                );
                Ok(())
            }
            TextInputAction::BookmarkRenameTo { old_name } => {
                self.apply_bookmark_rename_from_input(old_name, value)
            }
            TextInputAction::BookmarkSet => self.apply_bookmark_set_from_input(value),
            TextInputAction::BookmarkTrack => self.apply_bookmark_track_from_input(value),
            TextInputAction::BookmarkUntrack => self.apply_bookmark_untrack_from_input(value),
            TextInputAction::EditTarget => self.apply_edit_target_from_input(value),
            TextInputAction::FileTrack => self.apply_file_track_from_input(value),
            TextInputAction::GitFetchBranch => self.apply_git_fetch_from_input(Some("-b"), value),
            TextInputAction::GitFetchRemote => {
                self.apply_git_fetch_from_input(Some("--remote"), value)
            }
            TextInputAction::GitPushNamed { change_id } => {
                self.apply_git_push_named_from_input(change_id, value)
            }
            TextInputAction::GitPushBookmark => self.apply_git_push_from_input(Some("-b"), value),
            TextInputAction::MetaeditAuthor { change_id } => {
                self.apply_metaedit_from_input(change_id, "--author", value)
            }
            TextInputAction::MetaeditAuthorTimestamp { change_id } => {
                self.apply_metaedit_from_input(change_id, "--author-timestamp", value)
            }
            TextInputAction::NewAtTarget => self.apply_new_at_target_from_input(value),
            TextInputAction::NextPrevOffset { direction, mode } => {
                self.apply_next_prev_from_input(direction, mode, value)
            }
            TextInputAction::ParallelizeRevset => self.apply_parallelize_from_input(value),
            TextInputAction::RebaseTarget {
                source_type,
                destination_type,
            } => self.apply_rebase_target_from_input(source_type, destination_type, value),
            TextInputAction::SelectInRevset => {
                if let Ok(idx) = value.parse::<usize>() {
                    self.log_select(idx);
                }
                Ok(())
            }
            TextInputAction::WorkspaceAddPathOnly => {
                self.apply_workspace_add_from_input(value, None)
            }
            TextInputAction::WorkspaceAddNamePrompt => {
                self.start_text_input(
                    "Workspace path",
                    "",
                    TextInputAction::WorkspaceAddPathPrompt { name: value },
                );
                Ok(())
            }
            TextInputAction::WorkspaceAddPathPrompt { name } => {
                self.apply_workspace_add_from_input(value, Some(name))
            }
            TextInputAction::WorkspaceForget => self.apply_workspace_forget_from_input(value),
            TextInputAction::WorkspaceList => Ok(()),
            TextInputAction::WorkspaceRename => self.apply_workspace_rename_from_input(value),
        }
    }

    pub fn show_help(&mut self) {
        self.info_list = Some(self.command_tree.get_help());
    }

    pub fn handle_command_key(&mut self, key_code: KeyCode) -> Option<Message> {
        self.command_keys.push(key_code);

        let node = match self.command_tree.get_node(&self.command_keys) {
            None => {
                self.command_keys.pop();
                display_unbound_error_lines(&mut self.info_list, &key_code);
                return None;
            }
            Some(node) => node,
        };
        if let Some(children) = &node.children {
            self.info_list = Some(children.get_help());
        }
        if let Some(message) = node.action {
            if node.children.is_none() {
                self.command_keys.clear();
            }
            return Some(message);
        }
        None
    }

    pub fn scroll_down_once(&mut self) {
        if self.log_selected() <= self.log_offset() + self.log_list_scroll_padding {
            self.select_next_node();
        }
        *self.log_list_state.offset_mut() = self.log_offset() + 1;
    }

    pub fn scroll_up_once(&mut self) {
        if self.log_offset() == 0 {
            return;
        }
        let last_node_visible = self.line_dist_to_dest_node(
            self.log_list_layout.height as usize - 1,
            self.log_offset(),
            &ScrollDirection::Down,
        );
        if self.log_selected() >= last_node_visible - 1 - self.log_list_scroll_padding {
            self.select_prev_node();
        }
        *self.log_list_state.offset_mut() = self.log_offset().saturating_sub(1);
    }

    pub fn scroll_down_page(&mut self) {
        self.scroll_lines(self.log_list_layout.height as usize, &ScrollDirection::Down);
    }

    pub fn scroll_up_page(&mut self) {
        self.scroll_lines(self.log_list_layout.height as usize, &ScrollDirection::Up);
    }

    fn scroll_lines(&mut self, num_lines: usize, direction: &ScrollDirection) {
        let selected_node_dist_from_offset = self.log_selected() - self.log_offset();
        let mut target_offset =
            self.line_dist_to_dest_node(num_lines, self.log_offset(), direction);
        let mut target_node = target_offset + selected_node_dist_from_offset;
        match direction {
            ScrollDirection::Down => {
                if target_offset == self.log_list.len() - 1 {
                    target_node = target_offset;
                    target_offset = self.log_offset();
                }
            }
            ScrollDirection::Up => {
                // If we're already at the top of the page, then move selection to the top as well
                if target_offset == 0 && target_offset == self.log_offset() {
                    target_node = 0;
                }
            }
        }
        self.log_select(target_node);
        *self.log_list_state.offset_mut() = target_offset;
    }

    pub fn handle_mouse_click(&mut self, row: u16, column: u16) {
        let Rect {
            x,
            y,
            width,
            height,
        } = self.log_list_layout;

        // Check if inside log list
        if row < y || row >= y + height || column < x || column >= x + width {
            return;
        }

        let target_node = self.line_dist_to_dest_node(
            row as usize - y as usize,
            self.log_offset(),
            &ScrollDirection::Down,
        );
        self.log_select(target_node);
    }

    // Since some nodes contain multiple lines, we need a way to determine the destination node
    // which is n lines away from the starting node.
    fn line_dist_to_dest_node(
        &self,
        line_dist: usize,
        starting_node: usize,
        direction: &ScrollDirection,
    ) -> usize {
        let mut current_node = starting_node;
        let mut lines_traversed = 0;
        loop {
            let lines_in_node = self.log_list[current_node].lines.len();
            lines_traversed += lines_in_node;

            // Stop if we've found the dest node or have no further to traverse
            if match direction {
                ScrollDirection::Down => current_node == self.log_list.len() - 1,
                ScrollDirection::Up => current_node == 0,
            } || lines_traversed > line_dist
            {
                break;
            }

            match direction {
                ScrollDirection::Down => current_node += 1,
                ScrollDirection::Up => current_node -= 1,
            }
        }

        current_node
    }

    pub fn save_selection(&mut self) -> Result<()> {
        let Some(change_id) = self.get_selected_change_id() else {
            self.clear();
            return self.invalid_selection();
        };
        self.saved_change_id = Some(change_id.to_string());
        self.saved_file_path = self.get_selected_file_path().map(String::from);
        self.saved_tree_position = Some(self.get_selected_tree_position());

        Ok(())
    }

    pub fn open_file(&mut self, term: Term) -> Result<()> {
        let Some(file_path) = self.get_selected_file_path().map(|s| s.to_string()) else {
            return self.invalid_selection();
        };

        if self.is_selected_working_copy() {
            let full_path = format!("{}/{}", self.global_args.repository, file_path);
            open_file_in_editor(term, &full_path)?;
            self.info_list = Some(Text::from(format!("Opened {file_path}")));
            return Ok(());
        }

        let Some(change_id) = self.get_selected_change_id().map(|s| s.to_string()) else {
            return self.invalid_selection();
        };
        let Some(commit_id) = self.get_selected_commit_id().map(|s| s.to_string()) else {
            return self.invalid_selection();
        };

        let cmd = JjCommand::jj_file_show(&change_id, &file_path, self.global_args.clone());
        let contents = cmd.run().map_err(|e| anyhow::anyhow!("{}", e))?;

        let temp_dir = tempfile::Builder::new()
            .prefix(&format!("majjit-{change_id}-{commit_id}-"))
            .tempdir()?;
        let target_path = temp_dir.path().join(&file_path);
        if let Some(parent) = target_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&target_path, contents)?;
        let target_path_str = target_path.to_string_lossy().to_string();

        open_file_in_editor(term, &target_path_str)?;
        self.info_list = Some(Text::from(format!(
            "Opened {file_path} @ {change_id} (read-only copy)"
        )));
        Ok(())
    }

    pub fn jj_abandon(&mut self, mode: AbandonMode) -> Result<()> {
        let Some(change_id) = self.get_selected_change_id() else {
            return self.invalid_selection();
        };
        let mode = match mode {
            AbandonMode::Default => None,
            AbandonMode::RetainBookmarks => Some("--retain-bookmarks"),
            AbandonMode::RestoreDescendants => Some("--restore-descendants"),
        };
        let cmd = JjCommand::jj_abandon(change_id, mode, self.global_args.clone());
        self.queue_jj_command(cmd)
    }

    pub fn jj_absorb(&mut self, mode: AbsorbMode) -> Result<()> {
        let (from_change_id, maybe_into_change_id, maybe_file_path) = match mode {
            AbsorbMode::Default => {
                let Some(from_change_id) = self.get_selected_change_id() else {
                    return self.invalid_selection();
                };
                (from_change_id, None, self.get_selected_file_path())
            }
            AbsorbMode::Into => {
                let Some(from_change_id) = self.get_saved_change_id() else {
                    return self.invalid_selection();
                };
                let Some(into_change_id) = self.get_selected_change_id() else {
                    return self.invalid_selection();
                };
                (
                    from_change_id,
                    Some(into_change_id),
                    self.get_saved_file_path(),
                )
            }
        };

        let cmd = JjCommand::jj_absorb(
            from_change_id,
            maybe_into_change_id,
            maybe_file_path,
            self.global_args.clone(),
        );
        self.queue_jj_command(cmd)
    }

    fn apply_bookmark_create_from_input(&mut self, bookmark_names: String) -> Result<()> {
        let Some(change_id) = self.get_selected_change_id() else {
            return self.invalid_selection();
        };
        let cmd =
            JjCommand::jj_bookmark_create(&bookmark_names, change_id, self.global_args.clone());
        self.queue_jj_command(cmd)
    }

    pub fn jj_bookmark_create(&mut self) -> Result<()> {
        if self.get_selected_change_id().is_none() {
            return self.invalid_selection();
        }
        self.start_text_input("Bookmark create", "", TextInputAction::BookmarkCreate);
        Ok(())
    }

    fn apply_bookmark_delete_from_input(&mut self, bookmark_names: String) -> Result<()> {
        let cmd = JjCommand::jj_bookmark_delete(&bookmark_names, self.global_args.clone());
        self.queue_jj_command(cmd)
    }

    pub fn jj_bookmark_delete(&mut self) -> Result<()> {
        let bookmarks = self.get_bookmark_names()?;
        let candidates = bookmarks
            .into_iter()
            .map(FuzzyCandidate::from_display)
            .collect();
        self.start_fuzzy_input(
            "Bookmark delete",
            candidates,
            TextInputAction::BookmarkDelete,
        );
        Ok(())
    }

    fn apply_bookmark_forget_from_input(
        &mut self,
        bookmark_names: String,
        include_remotes: bool,
    ) -> Result<()> {
        let cmd = JjCommand::jj_bookmark_forget(
            &bookmark_names,
            include_remotes,
            self.global_args.clone(),
        );
        self.queue_jj_command(cmd)
    }

    pub fn jj_bookmark_forget(&mut self, include_remotes: bool) -> Result<()> {
        let bookmarks = self.get_bookmark_names()?;
        let candidates = bookmarks
            .into_iter()
            .map(FuzzyCandidate::from_display)
            .collect();
        self.start_fuzzy_input(
            "Bookmark forget",
            candidates,
            TextInputAction::BookmarkForget { include_remotes },
        );
        Ok(())
    }

    pub fn jj_bookmark_move(&mut self, mode: BookmarkMoveMode) -> Result<()> {
        let (from_change_id, to_change_id, allow_backwards) = match mode {
            BookmarkMoveMode::Default => {
                let Some(from_change_id) = self.get_saved_change_id() else {
                    return self.invalid_selection();
                };
                let Some(to_change_id) = self.get_selected_change_id() else {
                    return self.invalid_selection();
                };
                (from_change_id, to_change_id, false)
            }
            BookmarkMoveMode::AllowBackwards => {
                let Some(from_change_id) = self.get_saved_change_id() else {
                    return self.invalid_selection();
                };
                let Some(to_change_id) = self.get_selected_change_id() else {
                    return self.invalid_selection();
                };
                (from_change_id, to_change_id, true)
            }
            BookmarkMoveMode::Tug => {
                let Some(to_change_id) = self.get_selected_change_id() else {
                    return self.invalid_selection();
                };
                ("heads(::@- & bookmarks())", to_change_id, false)
            }
        };
        let cmd = JjCommand::jj_bookmark_move(
            from_change_id,
            to_change_id,
            allow_backwards,
            self.global_args.clone(),
        );
        self.queue_jj_command(cmd)
    }

    fn apply_bookmark_rename_from_input(
        &mut self,
        old_bookmark_name: String,
        new_bookmark_name: String,
    ) -> Result<()> {
        let cmd = JjCommand::jj_bookmark_rename(
            &old_bookmark_name,
            &new_bookmark_name,
            self.global_args.clone(),
        );
        self.queue_jj_command(cmd)
    }

    pub fn jj_bookmark_rename(&mut self) -> Result<()> {
        let bookmarks = self.get_bookmark_names()?;
        let candidates = bookmarks
            .into_iter()
            .map(FuzzyCandidate::from_display)
            .collect();
        self.start_fuzzy_input(
            "Bookmark rename from",
            candidates,
            TextInputAction::BookmarkRenameFrom,
        );
        Ok(())
    }

    fn apply_bookmark_set_from_input(&mut self, bookmark_names: String) -> Result<()> {
        let Some(change_id) = self.get_selected_change_id() else {
            return self.invalid_selection();
        };
        let cmd = JjCommand::jj_bookmark_set(&bookmark_names, change_id, self.global_args.clone());
        self.queue_jj_command(cmd)
    }

    pub fn jj_bookmark_set(&mut self) -> Result<()> {
        if self.get_selected_change_id().is_none() {
            return self.invalid_selection();
        }
        let bookmarks = self.get_bookmark_names()?;
        let candidates = bookmarks
            .into_iter()
            .map(FuzzyCandidate::from_display)
            .collect();
        self.start_fuzzy_input("Bookmark set", candidates, TextInputAction::BookmarkSet);
        Ok(())
    }

    fn apply_bookmark_track_from_input(&mut self, bookmark_at_remote: String) -> Result<()> {
        let cmd = JjCommand::jj_bookmark_track(&bookmark_at_remote, self.global_args.clone());
        self.queue_jj_command(cmd)
    }

    pub fn jj_bookmark_track(&mut self) -> Result<()> {
        let bookmarks = self.get_untracked_remote_bookmarks()?;
        let candidates = bookmarks
            .into_iter()
            .map(FuzzyCandidate::from_display)
            .collect();
        self.start_fuzzy_input("Bookmark track", candidates, TextInputAction::BookmarkTrack);
        Ok(())
    }

    fn apply_bookmark_untrack_from_input(&mut self, bookmark_at_remote: String) -> Result<()> {
        let cmd = JjCommand::jj_bookmark_untrack(&bookmark_at_remote, self.global_args.clone());
        self.queue_jj_command(cmd)
    }

    pub fn jj_bookmark_untrack(&mut self) -> Result<()> {
        let bookmarks = self.get_tracked_remote_bookmarks()?;
        let candidates = bookmarks
            .into_iter()
            .map(FuzzyCandidate::from_display)
            .collect();
        self.start_fuzzy_input(
            "Bookmark untrack",
            candidates,
            TextInputAction::BookmarkUntrack,
        );
        Ok(())
    }

    pub fn jj_commit(&mut self, term: Term) -> Result<()> {
        let maybe_file_path = self.get_selected_file_path();
        let cmd = JjCommand::jj_commit(maybe_file_path, self.global_args.clone(), term);
        self.queue_jj_command(cmd)
    }

    pub fn jj_describe(&mut self, term: Term) -> Result<()> {
        let Some(change_id) = self.get_selected_change_id() else {
            return self.invalid_selection();
        };
        let cmd = JjCommand::jj_describe(change_id, self.global_args.clone(), term);
        self.queue_jj_command(cmd)
    }

    pub fn jj_duplicate(
        &mut self,
        destination_type: DuplicateDestinationType,
        destination: DuplicateDestination,
    ) -> Result<()> {
        let destination_type = match destination_type {
            DuplicateDestinationType::Default => None,
            DuplicateDestinationType::Onto => Some("--onto"),
            DuplicateDestinationType::InsertAfter => Some("--insert-after"),
            DuplicateDestinationType::InsertBefore => Some("--insert-before"),
        };

        let change_id = if destination_type.is_some() {
            let Some(change_id) = self.get_saved_change_id() else {
                return self.invalid_selection();
            };
            change_id
        } else {
            let Some(change_id) = self.get_selected_change_id() else {
                return self.invalid_selection();
            };
            change_id
        };

        let destination = match destination {
            DuplicateDestination::Default => None,
            DuplicateDestination::Selection => {
                let Some(dest_change_id) = self.get_selected_change_id() else {
                    return self.invalid_selection();
                };
                Some(dest_change_id)
            }
        };

        let cmd = JjCommand::jj_duplicate(
            change_id,
            destination_type,
            destination,
            self.global_args.clone(),
        );
        self.queue_jj_command(cmd)
    }

    pub fn jj_edit(&mut self) -> Result<()> {
        let Some(change_id) = self.get_selected_change_id() else {
            return self.invalid_selection();
        };
        let cmd = JjCommand::jj_edit(change_id, self.global_args.clone());
        self.queue_jj_command(cmd)
    }

    fn apply_edit_target_from_input(&mut self, target: String) -> Result<()> {
        let cmd = JjCommand::jj_edit(&target, self.global_args.clone());
        self.queue_jj_command(cmd)
    }

    pub fn jj_edit_target(&mut self) -> Result<()> {
        let targets = self.get_revision_targets()?;
        let candidates = targets
            .into_iter()
            .map(FuzzyCandidate::from_display)
            .collect();
        self.start_fuzzy_input("Edit", candidates, TextInputAction::EditTarget);
        Ok(())
    }

    pub fn jj_evolog(&mut self, patch: bool, term: Term) -> Result<()> {
        let Some(change_id) = self.get_selected_change_id() else {
            return self.invalid_selection();
        };
        let cmd = JjCommand::jj_evolog(change_id, patch, self.global_args.clone(), term);
        self.queue_jj_command(cmd)
    }

    fn apply_file_track_from_input(&mut self, file_path: String) -> Result<()> {
        let cmd = JjCommand::jj_file_track(&file_path, self.global_args.clone());
        self.queue_jj_command(cmd)
    }

    pub fn jj_file_track(&mut self) -> Result<()> {
        let files = self.get_file_list()?;
        let candidates = files
            .into_iter()
            .map(FuzzyCandidate::from_display)
            .collect();
        self.start_fuzzy_input("File track", candidates, TextInputAction::FileTrack);
        Ok(())
    }

    pub fn jj_file_untrack(&mut self) -> Result<()> {
        let Some(file_path) = self.get_selected_file_path() else {
            return self.invalid_selection();
        };
        if !self.is_selected_working_copy() {
            return self.invalid_selection();
        }
        let cmd = JjCommand::jj_file_untrack(file_path, self.global_args.clone());
        self.queue_jj_command(cmd)
    }

    fn apply_git_fetch_from_input(&mut self, flag: Option<&str>, value: String) -> Result<()> {
        let cmd = JjCommand::jj_git_fetch(flag, Some(&value), self.global_args.clone());
        self.queue_jj_command(cmd)
    }

    pub fn jj_git_fetch(&mut self, mode: GitFetchMode) -> Result<()> {
        let (flag, value): (Option<&str>, Option<String>) = match mode {
            GitFetchMode::Default => (None, None),
            GitFetchMode::AllRemotes => (Some("--all-remotes"), None),
            GitFetchMode::Tracked => (Some("--tracked"), None),
            GitFetchMode::Branch => {
                let bookmarks = self.get_bookmark_names()?;
                let candidates = bookmarks
                    .into_iter()
                    .map(FuzzyCandidate::from_display)
                    .collect();
                self.start_fuzzy_input("Fetch branch", candidates, TextInputAction::GitFetchBranch);
                return Ok(());
            }
            GitFetchMode::Remote => {
                let remotes = self.get_git_remote_names()?;
                let candidates = remotes
                    .into_iter()
                    .map(FuzzyCandidate::from_display)
                    .collect();
                self.start_fuzzy_input("Fetch remote", candidates, TextInputAction::GitFetchRemote);
                return Ok(());
            }
        };
        let cmd = JjCommand::jj_git_fetch(flag, value.as_deref(), self.global_args.clone());
        self.queue_jj_command(cmd)
    }

    fn apply_git_push_named_from_input(
        &mut self,
        change_id: String,
        bookmark_name: String,
    ) -> Result<()> {
        let value = format!("{}={}", bookmark_name, change_id);
        let cmd = JjCommand::jj_git_push(Some("--named"), Some(&value), self.global_args.clone());
        self.queue_jj_command(cmd)
    }

    fn apply_git_push_from_input(&mut self, flag: Option<&str>, value: String) -> Result<()> {
        let cmd = JjCommand::jj_git_push(flag, Some(&value), self.global_args.clone());
        self.queue_jj_command(cmd)
    }

    pub fn jj_git_push(&mut self, mode: GitPushMode) -> Result<()> {
        let (flag, value) = match mode {
            GitPushMode::Default => (None, None),
            GitPushMode::All => (Some("--all"), None),
            GitPushMode::Tracked => (Some("--tracked"), None),
            GitPushMode::Deleted => (Some("--deleted"), None),
            GitPushMode::Revision => {
                let Some(change_id) = self.get_selected_change_id() else {
                    return self.invalid_selection();
                };
                (Some("-r"), Some(change_id.to_string()))
            }
            GitPushMode::Change => {
                let Some(change_id) = self.get_selected_change_id() else {
                    return self.invalid_selection();
                };
                (Some("-c"), Some(change_id.to_string()))
            }
            GitPushMode::Named => {
                let Some(change_id) = self.get_selected_change_id() else {
                    return self.invalid_selection();
                };
                self.start_text_input(
                    "Bookmark name",
                    "",
                    TextInputAction::GitPushNamed {
                        change_id: change_id.to_string(),
                    },
                );
                return Ok(());
            }
            GitPushMode::Bookmark => {
                let bookmarks = self.get_bookmark_names()?;
                let candidates = bookmarks
                    .into_iter()
                    .map(FuzzyCandidate::from_display)
                    .collect();
                self.start_fuzzy_input(
                    "Push bookmark",
                    candidates,
                    TextInputAction::GitPushBookmark,
                );
                return Ok(());
            }
        };
        let cmd = JjCommand::jj_git_push(flag, value.as_deref(), self.global_args.clone());
        self.queue_jj_command(cmd)
    }

    pub fn jj_interdiff(&mut self, mode: InterdiffMode, term: Term) -> Result<()> {
        let (from, to, maybe_file_path) = match mode {
            InterdiffMode::FromSelection => {
                let Some(from_change_id) = self.get_selected_change_id() else {
                    return self.invalid_selection();
                };
                (from_change_id, "@", self.get_selected_file_path())
            }
            InterdiffMode::FromSelectionToDestination => {
                let Some(from_change_id) = self.get_saved_change_id() else {
                    return self.invalid_selection();
                };
                let Some(to_change_id) = self.get_selected_change_id() else {
                    return self.invalid_selection();
                };
                (from_change_id, to_change_id, self.get_saved_file_path())
            }
            InterdiffMode::ToSelection => {
                let Some(to_change_id) = self.get_selected_change_id() else {
                    return self.invalid_selection();
                };
                ("@", to_change_id, self.get_selected_file_path())
            }
        };

        let cmd =
            JjCommand::jj_interdiff(from, to, maybe_file_path, self.global_args.clone(), term);
        self.queue_jj_command(cmd)
    }

    fn apply_metaedit_from_input(
        &mut self,
        change_id: String,
        flag: &str,
        value: String,
    ) -> Result<()> {
        let cmd = JjCommand::jj_metaedit(&change_id, flag, Some(&value), self.global_args.clone());
        self.queue_jj_command(cmd)
    }

    pub fn jj_metaedit(&mut self, action: MetaeditAction) -> Result<()> {
        let Some(change_id) = self.get_selected_change_id() else {
            return self.invalid_selection();
        };

        let (flag, value): (&str, Option<String>) = match action {
            MetaeditAction::UpdateChangeId => ("--update-change-id", None),
            MetaeditAction::UpdateAuthorTimestamp => ("--update-author-timestamp", None),
            MetaeditAction::UpdateAuthor => ("--update-author", None),
            MetaeditAction::ForceRewrite => ("--force-rewrite", None),
            MetaeditAction::SetAuthor => {
                self.start_text_input(
                    "Author",
                    "",
                    TextInputAction::MetaeditAuthor {
                        change_id: change_id.to_string(),
                    },
                );
                return Ok(());
            }
            MetaeditAction::SetAuthorTimestamp => {
                self.start_text_input(
                    "Author timestamp",
                    "",
                    TextInputAction::MetaeditAuthorTimestamp {
                        change_id: change_id.to_string(),
                    },
                );
                return Ok(());
            }
        };

        let cmd =
            JjCommand::jj_metaedit(change_id, flag, value.as_deref(), self.global_args.clone());
        self.queue_jj_command(cmd)
    }

    pub fn jj_new(&mut self, mode: NewMode) -> Result<()> {
        let cmd = match mode {
            NewMode::Default => {
                let Some(change_id) = self.get_selected_change_id() else {
                    return self.invalid_selection();
                };
                JjCommand::jj_new(change_id, &[], self.global_args.clone())
            }
            NewMode::AfterTrunk => JjCommand::jj_new("trunk()", &[], self.global_args.clone()),
            NewMode::Before => {
                let Some(change_id) = self.get_selected_change_id() else {
                    return self.invalid_selection();
                };
                JjCommand::jj_new(
                    change_id,
                    &["--no-edit", "--insert-before"],
                    self.global_args.clone(),
                )
            }
            NewMode::InsertAfter => {
                let Some(change_id) = self.get_selected_change_id() else {
                    return self.invalid_selection();
                };
                JjCommand::jj_new(change_id, &["--insert-after"], self.global_args.clone())
            }
        };
        self.queue_jj_command(cmd)
    }

    pub fn jj_new_after_trunk_sync(&mut self) -> Result<()> {
        let fetch_cmd = JjCommand::jj_git_fetch(None, None, self.global_args.clone());
        let new_cmd = JjCommand::jj_new("trunk()", &[], self.global_args.clone());
        self.queue_jj_commands(vec![fetch_cmd, new_cmd])
    }

    fn apply_new_at_target_from_input(&mut self, target: String) -> Result<()> {
        let cmd = JjCommand::jj_new(&target, &[], self.global_args.clone());
        self.queue_jj_command(cmd)
    }

    pub fn jj_new_at_target(&mut self) -> Result<()> {
        let targets = self.get_revision_targets()?;
        let candidates = targets
            .into_iter()
            .map(FuzzyCandidate::from_display)
            .collect();
        self.start_fuzzy_input("New after", candidates, TextInputAction::NewAtTarget);
        Ok(())
    }

    fn apply_next_prev_from_input(
        &mut self,
        direction: NextPrevDirection,
        mode: NextPrevMode,
        offset: String,
    ) -> Result<()> {
        let mode = match mode {
            NextPrevMode::Conflict => Some("--conflict"),
            NextPrevMode::Default => None,
            NextPrevMode::Edit => Some("--edit"),
            NextPrevMode::NoEdit => Some("--no-edit"),
        };

        let direction = match direction {
            NextPrevDirection::Next => "next",
            NextPrevDirection::Prev => "prev",
        };
        let cmd = JjCommand::jj_next_prev(direction, mode, Some(&offset), self.global_args.clone());
        self.queue_jj_command(cmd)
    }

    pub fn jj_next_prev(
        &mut self,
        direction: NextPrevDirection,
        mode: NextPrevMode,
        offset: bool,
    ) -> Result<()> {
        if offset {
            self.start_text_input(
                "Offset",
                "",
                TextInputAction::NextPrevOffset { direction, mode },
            );
            return Ok(());
        }

        let mode = match mode {
            NextPrevMode::Conflict => Some("--conflict"),
            NextPrevMode::Default => None,
            NextPrevMode::Edit => Some("--edit"),
            NextPrevMode::NoEdit => Some("--no-edit"),
        };

        let direction = match direction {
            NextPrevDirection::Next => "next",
            NextPrevDirection::Prev => "prev",
        };
        let cmd = JjCommand::jj_next_prev(direction, mode, None, self.global_args.clone());
        self.queue_jj_command(cmd)
    }

    fn apply_parallelize_from_input(&mut self, revset: String) -> Result<()> {
        let cmd = JjCommand::jj_parallelize(&revset, self.global_args.clone());
        self.queue_jj_command(cmd)
    }

    pub fn jj_parallelize(&mut self, source: ParallelizeSource) -> Result<()> {
        let revset = match source {
            ParallelizeSource::Range => {
                let Some(from_change_id) = self.get_saved_change_id() else {
                    return self.invalid_selection();
                };
                let Some(to_change_id) = self.get_selected_change_id() else {
                    return self.invalid_selection();
                };
                format!("{}::{}", from_change_id, to_change_id)
            }
            ParallelizeSource::Revset => {
                self.start_text_input("Parallelize revset", "", TextInputAction::ParallelizeRevset);
                return Ok(());
            }
            ParallelizeSource::Selection => {
                let Some(change_id) = self.get_selected_change_id() else {
                    return self.invalid_selection();
                };
                format!("{}-::{}", change_id, change_id)
            }
        };
        let cmd = JjCommand::jj_parallelize(&revset, self.global_args.clone());
        self.queue_jj_command(cmd)
    }

    pub fn jj_rebase(
        &mut self,
        source_type: RebaseSourceType,
        destination_type: RebaseDestinationType,
        destination: RebaseDestination,
    ) -> Result<()> {
        let Some(source_change_id) = self.get_saved_change_id() else {
            return self.invalid_selection();
        };
        let source_type = match source_type {
            RebaseSourceType::Branch => "--branch",
            RebaseSourceType::Source => "--source",
            RebaseSourceType::Revisions => "--revisions",
        };
        let destination_type = match destination_type {
            RebaseDestinationType::InsertAfter => "--insert-after",
            RebaseDestinationType::InsertBefore => "--insert-before",
            RebaseDestinationType::Onto => "--onto",
        };
        let destination = match destination {
            RebaseDestination::Selection => {
                let Some(dest_change_id) = self.get_selected_change_id() else {
                    return self.invalid_selection();
                };
                dest_change_id
            }
            RebaseDestination::Trunk => "trunk()",
            RebaseDestination::Current => "@",
        };

        let cmd = JjCommand::jj_rebase(
            source_type,
            source_change_id,
            destination_type,
            destination,
            self.global_args.clone(),
        );
        self.queue_jj_command(cmd)
    }

    pub fn jj_rebase_target_fuzzy(
        &mut self,
        source_type: RebaseSourceType,
        destination_type: RebaseDestinationType,
    ) -> Result<()> {
        let candidates = self.log_revset_candidates(false);
        self.start_fuzzy_input(
            "Rebase target",
            candidates,
            TextInputAction::RebaseTarget {
                source_type,
                destination_type,
            },
        );
        Ok(())
    }

    fn apply_rebase_target_from_input(
        &mut self,
        source_type: RebaseSourceType,
        destination_type: RebaseDestinationType,
        destination: String,
    ) -> Result<()> {
        let Some(source_change_id) = self.get_saved_change_id() else {
            return self.invalid_selection();
        };
        let source_type = match source_type {
            RebaseSourceType::Branch => "--branch",
            RebaseSourceType::Source => "--source",
            RebaseSourceType::Revisions => "--revisions",
        };
        let destination_type = match destination_type {
            RebaseDestinationType::InsertAfter => "--insert-after",
            RebaseDestinationType::InsertBefore => "--insert-before",
            RebaseDestinationType::Onto => "--onto",
        };
        let cmd = JjCommand::jj_rebase(
            source_type,
            source_change_id,
            destination_type,
            &destination,
            self.global_args.clone(),
        );
        self.queue_jj_command(cmd)
    }

    pub fn jj_rebase_selected_branch_onto_trunk(&mut self) -> Result<()> {
        let Some(source_change_id) = self.get_selected_change_id() else {
            return self.invalid_selection();
        };

        let cmd = JjCommand::jj_rebase(
            "--branch",
            source_change_id,
            "--onto",
            "trunk()",
            self.global_args.clone(),
        );
        self.queue_jj_command(cmd)
    }

    pub fn jj_rebase_selected_branch_onto_trunk_sync(&mut self) -> Result<()> {
        let Some(source_change_id) = self.get_selected_change_id() else {
            return self.invalid_selection();
        };

        let fetch_cmd = JjCommand::jj_git_fetch(None, None, self.global_args.clone());
        let rebase_cmd = JjCommand::jj_rebase(
            "--branch",
            source_change_id,
            "--onto",
            "trunk()",
            self.global_args.clone(),
        );
        self.queue_jj_commands(vec![fetch_cmd, rebase_cmd])
    }

    pub fn jj_redo(&mut self) -> Result<()> {
        let cmd = JjCommand::jj_redo(self.global_args.clone());
        self.queue_jj_command(cmd)
    }

    pub fn jj_resolve(&mut self, term: Term) -> Result<()> {
        let Some(change_id) = self.get_selected_change_id() else {
            return self.invalid_selection();
        };
        let maybe_file_path = self.get_selected_file_path();
        let cmd = JjCommand::jj_resolve(change_id, maybe_file_path, self.global_args.clone(), term);
        self.queue_jj_command(cmd)
    }

    pub fn jj_restore(&mut self, mode: RestoreMode) -> Result<()> {
        let (flags, maybe_file_path) = match mode {
            RestoreMode::ChangesIn => {
                let Some(change_id) = self.get_selected_change_id() else {
                    return self.invalid_selection();
                };
                (
                    vec!["--changes-in", change_id],
                    self.get_selected_file_path(),
                )
            }
            RestoreMode::ChangesInRestoreDescendants => {
                let Some(change_id) = self.get_selected_change_id() else {
                    return self.invalid_selection();
                };
                (
                    vec!["--changes-in", change_id, "--restore-descendants"],
                    self.get_selected_file_path(),
                )
            }
            RestoreMode::From => {
                let Some(change_id) = self.get_selected_change_id() else {
                    return self.invalid_selection();
                };
                (vec!["--from", change_id], self.get_selected_file_path())
            }
            RestoreMode::Into => {
                let Some(change_id) = self.get_selected_change_id() else {
                    return self.invalid_selection();
                };
                (vec!["--into", change_id], self.get_selected_file_path())
            }
            RestoreMode::FromInto => {
                let Some(from_change_id) = self.get_saved_change_id() else {
                    return self.invalid_selection();
                };
                let Some(into_change_id) = self.get_selected_change_id() else {
                    return self.invalid_selection();
                };
                (
                    vec!["--from", from_change_id, "--into", into_change_id],
                    self.get_saved_file_path(),
                )
            }
        };

        let cmd = JjCommand::jj_restore(&flags, maybe_file_path, self.global_args.clone());
        self.queue_jj_command(cmd)
    }

    pub fn jj_revert(
        &mut self,
        revision: RevertRevision,
        destination_type: RevertDestinationType,
        destination: RevertDestination,
    ) -> Result<()> {
        let revision = match revision {
            RevertRevision::Saved => {
                let Some(revision) = self.get_saved_change_id() else {
                    return self.invalid_selection();
                };
                revision
            }
            RevertRevision::Selection => {
                let Some(revision) = self.get_selected_change_id() else {
                    return self.invalid_selection();
                };
                revision
            }
        };
        let destination_type = match destination_type {
            RevertDestinationType::Onto => "--onto",
            RevertDestinationType::InsertAfter => "--insert-after",
            RevertDestinationType::InsertBefore => "--insert-before",
        };
        let destination = match destination {
            RevertDestination::Current => "@",
            RevertDestination::Selection => {
                let Some(destination) = self.get_selected_change_id() else {
                    return self.invalid_selection();
                };
                destination
            }
        };

        let cmd = JjCommand::jj_revert(
            revision,
            destination_type,
            destination,
            self.global_args.clone(),
        );
        self.queue_jj_command(cmd)
    }

    pub fn jj_sign(&mut self, action: SignAction, range: bool) -> Result<()> {
        let revset = if range {
            let Some(from_change_id) = self.get_saved_change_id() else {
                return self.invalid_selection();
            };
            let Some(to_change_id) = self.get_selected_change_id() else {
                return self.invalid_selection();
            };
            format!("{}::{}", from_change_id, to_change_id)
        } else {
            let Some(change_id) = self.get_selected_change_id() else {
                return self.invalid_selection();
            };
            change_id.to_string()
        };

        let action = match action {
            SignAction::Sign => "sign",
            SignAction::Unsign => "unsign",
        };
        let cmd = JjCommand::jj_sign(action, &revset, self.global_args.clone());
        self.queue_jj_command(cmd)
    }

    pub fn jj_simplify_parents(&mut self, mode: SimplifyParentsMode) -> Result<()> {
        let Some(change_id) = self.get_selected_change_id() else {
            return self.invalid_selection();
        };
        let mode = match mode {
            SimplifyParentsMode::Revisions => "-r",
            SimplifyParentsMode::Source => "-s",
        };
        let cmd = JjCommand::jj_simplify_parents(change_id, mode, self.global_args.clone());
        self.queue_jj_command(cmd)
    }

    pub fn jj_squash(&mut self, mode: SquashMode, term: Term) -> Result<()> {
        let cmd = match mode {
            SquashMode::Default => {
                let tree_pos = self.get_selected_tree_position();
                let Some(commit) = self.jj_log.get_tree_commit(&tree_pos) else {
                    return self.invalid_selection();
                };
                let maybe_file_path = self.get_selected_file_path();

                if commit.description_first_line.is_none() {
                    JjCommand::jj_squash_noninteractive(
                        &commit.change_id,
                        maybe_file_path,
                        self.global_args.clone(),
                    )
                } else {
                    JjCommand::jj_squash_interactive(
                        &commit.change_id,
                        maybe_file_path,
                        self.global_args.clone(),
                        term,
                    )
                }
            }
            SquashMode::Into => {
                let Some(from_change_id) = self.get_saved_change_id() else {
                    return self.invalid_selection();
                };
                let maybe_file_path = self.get_saved_file_path();
                let Some(into_change_id) = self.get_selected_change_id() else {
                    return self.invalid_selection();
                };
                JjCommand::jj_squash_into_interactive(
                    from_change_id,
                    into_change_id,
                    maybe_file_path,
                    self.global_args.clone(),
                    term,
                )
            }
        };

        self.queue_jj_command(cmd)
    }

    pub fn jj_status(&mut self, term: Term) -> Result<()> {
        let cmd = JjCommand::jj_status(self.global_args.clone(), term);
        self.queue_jj_command(cmd)
    }

    pub fn jj_undo(&mut self) -> Result<()> {
        let cmd = JjCommand::jj_undo(self.global_args.clone());
        self.queue_jj_command(cmd)
    }

    pub fn jj_view(&mut self, mode: ViewMode, term: Term) -> Result<()> {
        let cmd = match mode {
            ViewMode::Default => {
                let Some(change_id) = self.get_selected_change_id() else {
                    return self.invalid_selection();
                };
                match self.get_selected_file_path() {
                    Some(file_path) => JjCommand::jj_diff_file_interactive(
                        change_id,
                        file_path,
                        self.global_args.clone(),
                        term,
                    ),
                    None => JjCommand::jj_show(change_id, self.global_args.clone(), term),
                }
            }
            ViewMode::FromSelection => {
                let Some(from_change_id) = self.get_selected_change_id() else {
                    return self.invalid_selection();
                };
                let file = self.get_selected_file_path();
                JjCommand::jj_diff_from_to_interactive(
                    from_change_id,
                    "@",
                    file,
                    self.global_args.clone(),
                    term,
                )
            }
            ViewMode::FromSelectionToDestination => {
                let Some(from_change_id) = self.get_saved_change_id() else {
                    return self.invalid_selection();
                };
                let Some(to_change_id) = self.get_selected_change_id() else {
                    return self.invalid_selection();
                };
                let file = self.get_selected_file_path();
                JjCommand::jj_diff_from_to_interactive(
                    from_change_id,
                    to_change_id,
                    file,
                    self.global_args.clone(),
                    term,
                )
            }
            ViewMode::FromTrunkToSelection => {
                let Some(to_change_id) = self.get_selected_change_id() else {
                    return self.invalid_selection();
                };
                let file = self.get_selected_file_path();
                JjCommand::jj_diff_from_to_interactive(
                    "trunk()",
                    to_change_id,
                    file,
                    self.global_args.clone(),
                    term,
                )
            }
            ViewMode::ToSelection => {
                let Some(to_change_id) = self.get_selected_change_id() else {
                    return self.invalid_selection();
                };
                let file = self.get_selected_file_path();
                JjCommand::jj_diff_from_to_interactive(
                    "@",
                    to_change_id,
                    file,
                    self.global_args.clone(),
                    term,
                )
            }
        };
        self.queue_jj_command(cmd)
    }

    fn apply_workspace_add_from_input(&mut self, path: String, name: Option<String>) -> Result<()> {
        if path.is_empty() {
            return self.cancelled();
        }
        let name_ref = name.as_deref().filter(|s| !s.is_empty());
        let cmd = JjCommand::jj_workspace_add(&path, name_ref, self.global_args.clone());
        self.queue_jj_command(cmd)
    }

    pub fn jj_workspace_add_path_only(&mut self) -> Result<()> {
        self.start_text_input("Workspace path", "", TextInputAction::WorkspaceAddPathOnly);
        Ok(())
    }

    pub fn jj_workspace_add_named(&mut self) -> Result<()> {
        self.start_text_input(
            "Workspace name",
            "",
            TextInputAction::WorkspaceAddNamePrompt,
        );
        Ok(())
    }

    fn apply_workspace_forget_from_input(&mut self, name: String) -> Result<()> {
        if name.is_empty() {
            return self.cancelled();
        }
        let cmd = JjCommand::jj_workspace_forget(&[&name], self.global_args.clone());
        self.queue_jj_command(cmd)
    }

    pub fn jj_workspace_forget_current(&mut self) -> Result<()> {
        let cmd = JjCommand::jj_workspace_forget(&[], self.global_args.clone());
        self.queue_jj_command(cmd)
    }

    pub fn jj_workspace_forget_fuzzy(&mut self) -> Result<()> {
        let workspaces = self.get_workspace_names()?;
        let candidates = workspaces
            .into_iter()
            .map(FuzzyCandidate::from_display)
            .collect();
        self.start_fuzzy_input(
            "Workspace forget",
            candidates,
            TextInputAction::WorkspaceForget,
        );
        Ok(())
    }

    pub fn jj_workspace_forget_at_selection(&mut self) -> Result<()> {
        let workspaces = self.get_selected_workspaces();
        if workspaces.is_empty() {
            return self.invalid_selection();
        }
        let refs: Vec<&str> = workspaces.iter().map(String::as_str).collect();
        let cmd = JjCommand::jj_workspace_forget(&refs, self.global_args.clone());
        self.queue_jj_command(cmd)
    }

    pub fn jj_workspace_list(&mut self) -> Result<()> {
        let workspaces = self.get_workspace_names()?;
        let candidates = workspaces
            .into_iter()
            .map(FuzzyCandidate::from_display)
            .collect();
        self.start_fuzzy_input("Workspaces", candidates, TextInputAction::WorkspaceList);
        Ok(())
    }

    fn apply_workspace_rename_from_input(&mut self, new_name: String) -> Result<()> {
        if new_name.is_empty() {
            return self.cancelled();
        }
        let cmd = JjCommand::jj_workspace_rename(&new_name, self.global_args.clone());
        self.queue_jj_command(cmd)
    }

    pub fn jj_workspace_rename(&mut self) -> Result<()> {
        let current = self.get_current_workspace_name()?;
        self.start_text_input(
            "Rename current workspace to",
            &current,
            TextInputAction::WorkspaceRename,
        );
        Ok(())
    }

    pub fn jj_workspace_update_stale(&mut self) -> Result<()> {
        let cmd = JjCommand::jj_workspace_update_stale(self.global_args.clone());
        self.queue_jj_command(cmd)
    }

    fn queue_jj_command(&mut self, cmd: JjCommand) -> Result<()> {
        self.queue_jj_commands(vec![cmd])
    }

    fn queue_jj_commands(&mut self, cmds: Vec<JjCommand>) -> Result<()> {
        self.accumulated_command_output.clear();
        self.queued_jj_commands = cmds;
        self.update_info_list_for_queue();
        Ok(())
    }

    fn update_info_list_for_queue(&mut self) {
        let mut lines = self.accumulated_command_output.clone();
        if let Some(cmd) = self.queued_jj_commands.first() {
            lines.extend(cmd.to_lines());
            lines.push(Line::raw("Running..."));
        }
        self.info_list = Some(Text::from(lines));
    }

    pub fn process_jj_command_queue(&mut self) -> Result<()> {
        if self.queued_jj_commands.is_empty() {
            return Ok(());
        }

        let cmd = self.queued_jj_commands.remove(0);
        let result = cmd.run();

        // Accumulate output from this command (with blank line separator)
        if !self.accumulated_command_output.is_empty() {
            self.accumulated_command_output.push(Line::raw(""));
        }
        self.accumulated_command_output.extend(cmd.to_lines());

        match result {
            Ok(output) => {
                self.accumulated_command_output
                    .extend(output.into_text()?.lines);

                if self.queued_jj_commands.is_empty() {
                    // All commands done, show final output and sync
                    let final_output = self.accumulated_command_output.clone();
                    self.clear();
                    self.info_list = Some(Text::from(final_output));
                    if cmd.sync {
                        self.sync()?;
                    }
                } else {
                    // More commands to run, update info_list to show next command
                    self.update_info_list_for_queue();
                }
            }
            Err(err) => match err {
                JjCommandError::Other { err } => return Err(err),
                JjCommandError::Failed { stderr } => {
                    // Command failed, show error with accumulated output
                    self.accumulated_command_output
                        .extend(stderr.into_text()?.lines);
                    let final_output = self.accumulated_command_output.clone();
                    self.clear();
                    self.info_list = Some(Text::from(final_output));
                }
            },
        }

        Ok(())
    }
}

fn format_repository_for_display(repository: &str) -> String {
    let Ok(home_dir) = std::env::var("HOME") else {
        return repository.to_string();
    };

    if repository == home_dir {
        return "~".to_string();
    }

    let home_prefix = format!("{home_dir}/");
    match repository.strip_prefix(&home_prefix) {
        Some(relative_path) => format!("~/{relative_path}"),
        None => repository.to_string(),
    }
}
