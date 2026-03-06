use crate::{
    command_tree::{CommandTree, display_unbound_error_lines},
    log_tree::{DIFF_HUNK_LINE_IDX, JjLog, LogTreeNode, TreePosition, get_parent_tree_position},
    shell_out::{JjCommand, JjCommandError, get_input_from_editor, open_file_in_editor},
    terminal::Term,
    update::{
        AbandonMode, AbsorbMode, BookmarkMoveMode, DuplicateDestination, DuplicateDestinationType,
        GitFetchMode, GitPushMode, InterdiffMode, Message, MetaeditAction, NewMode,
        NextPrevDirection, NextPrevMode, ParallelizeSource, RebaseDestination,
        RebaseDestinationType, RebaseSourceType, RestoreMode, RevertDestination,
        RevertDestinationType, RevertRevision, SignAction, SimplifyParentsMode, SquashMode,
        ViewMode,
    },
};
use ansi_to_tui::IntoText;
use anyhow::Result;
use crossterm::event::KeyCode;
use ratatui::{
    layout::Rect,
    text::{Line, Text},
    widgets::ListState,
};

pub const DEFAULT_REVSET: &str = "root() | remote_bookmarks() | ancestors(immutable_heads().., 24)";

const LOG_LIST_SCROLL_PADDING: usize = 0;

#[derive(Default, Debug, PartialEq, Eq)]
pub enum State {
    #[default]
    Running,
    Quit,
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
    pub info_list: Option<Text<'static>>,
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
            info_list: None,
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
            Some(commit) => commit.flat_log_idx,
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
            .map(|commit| commit.flat_log_idx);
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
            self.log_select(commit.flat_log_idx);
        }
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

    pub fn set_revset(&mut self, term: Term) -> Result<()> {
        let old_revset = self.revset.clone();
        let Some(new_revset) =
            get_input_from_editor(term, Some(&self.revset), Some("Enter the new revset"))?
        else {
            return self.cancelled();
        };
        self.revset = new_revset.clone();
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
        if !self.is_selected_working_copy() {
            return self.invalid_selection();
        }
        let Some(file_path) = self.get_selected_file_path() else {
            return self.invalid_selection();
        };
        let full_path = format!("{}/{}", self.global_args.repository, file_path);
        open_file_in_editor(term, &full_path)?;
        self.info_list = Some(Text::from(format!("Opened {file_path}")));
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
        let cmd = JjCommand::abandon(change_id, mode, self.global_args.clone());
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

        let cmd = JjCommand::absorb(
            from_change_id,
            maybe_into_change_id,
            maybe_file_path,
            self.global_args.clone(),
        );
        self.queue_jj_command(cmd)
    }

    pub fn jj_bookmark_create(&mut self, term: Term) -> Result<()> {
        let Some(change_id) = self.get_selected_change_id() else {
            return self.invalid_selection();
        };
        let Some(bookmark_names) =
            get_input_from_editor(term, None, Some("Enter the new bookmark(s)"))?
        else {
            return self.cancelled();
        };
        let cmd = JjCommand::bookmark_create(&bookmark_names, change_id, self.global_args.clone());
        self.queue_jj_command(cmd)
    }

    pub fn jj_bookmark_delete(&mut self, term: Term) -> Result<()> {
        let Some(bookmark_names) =
            get_input_from_editor(term, None, Some("Enter the bookmark(s) to delete"))?
        else {
            return self.cancelled();
        };
        let cmd = JjCommand::bookmark_delete(&bookmark_names, self.global_args.clone());
        self.queue_jj_command(cmd)
    }

    pub fn jj_bookmark_forget(&mut self, include_remotes: bool, term: Term) -> Result<()> {
        let prompt = if include_remotes {
            "Enter the bookmark(s) to forget, including remotes"
        } else {
            "Enter the bookmark(s) to forget"
        };
        let Some(bookmark_names) = get_input_from_editor(term, None, Some(prompt))? else {
            return self.cancelled();
        };
        let cmd =
            JjCommand::bookmark_forget(&bookmark_names, include_remotes, self.global_args.clone());
        self.queue_jj_command(cmd)
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
        let cmd = JjCommand::bookmark_move(
            from_change_id,
            to_change_id,
            allow_backwards,
            self.global_args.clone(),
        );
        self.queue_jj_command(cmd)
    }

    pub fn jj_bookmark_rename(&mut self, term: Term) -> Result<()> {
        let Some(old_bookmark_name) =
            get_input_from_editor(term.clone(), None, Some("Enter the bookmark to rename"))?
        else {
            return self.cancelled();
        };
        let Some(new_bookmark_name) =
            get_input_from_editor(term, None, Some("Enter the bookmark to rename to"))?
        else {
            return self.cancelled();
        };
        let cmd = JjCommand::bookmark_rename(
            &old_bookmark_name,
            &new_bookmark_name,
            self.global_args.clone(),
        );
        self.queue_jj_command(cmd)
    }

    pub fn jj_bookmark_set(&mut self, term: Term) -> Result<()> {
        let Some(change_id) = self.get_selected_change_id() else {
            return self.invalid_selection();
        };
        let Some(bookmark_names) =
            get_input_from_editor(term, None, Some("Enter the bookmark(s) to set"))?
        else {
            return self.cancelled();
        };
        let cmd = JjCommand::bookmark_set(&bookmark_names, change_id, self.global_args.clone());
        self.queue_jj_command(cmd)
    }

    pub fn jj_bookmark_track(&mut self, term: Term) -> Result<()> {
        let Some(bookmark_at_remote) =
            get_input_from_editor(term, None, Some("Enter the bookmark@remote to track"))?
        else {
            return self.cancelled();
        };
        let cmd = JjCommand::bookmark_track(&bookmark_at_remote, self.global_args.clone());
        self.queue_jj_command(cmd)
    }

    pub fn jj_bookmark_untrack(&mut self, term: Term) -> Result<()> {
        let Some(bookmark_at_remote) =
            get_input_from_editor(term, None, Some("Enter the bookmark@remote to untrack"))?
        else {
            return self.cancelled();
        };
        let cmd = JjCommand::bookmark_untrack(&bookmark_at_remote, self.global_args.clone());
        self.queue_jj_command(cmd)
    }

    pub fn jj_commit(&mut self, term: Term) -> Result<()> {
        let maybe_file_path = self.get_selected_file_path();
        let cmd = JjCommand::commit(maybe_file_path, self.global_args.clone(), term);
        self.queue_jj_command(cmd)
    }

    pub fn jj_describe(&mut self, term: Term) -> Result<()> {
        let Some(change_id) = self.get_selected_change_id() else {
            return self.invalid_selection();
        };
        let cmd = JjCommand::describe(change_id, self.global_args.clone(), term);
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

        let cmd = JjCommand::duplicate(
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
        let cmd = JjCommand::edit(change_id, self.global_args.clone());
        self.queue_jj_command(cmd)
    }

    pub fn jj_edit_target(&mut self, term: Term) -> Result<()> {
        let Some(target) = get_input_from_editor(term, None, Some("Enter the target to edit"))?
        else {
            return self.cancelled();
        };
        let cmd = JjCommand::edit(&target, self.global_args.clone());
        self.queue_jj_command(cmd)
    }

    pub fn jj_evolog(&mut self, patch: bool, term: Term) -> Result<()> {
        let Some(change_id) = self.get_selected_change_id() else {
            return self.invalid_selection();
        };
        let cmd = JjCommand::evolog(change_id, patch, self.global_args.clone(), term);
        self.queue_jj_command(cmd)
    }

    pub fn jj_file_track(&mut self, term: Term) -> Result<()> {
        let Some(file_path) =
            get_input_from_editor(term, None, Some("Enter the file path(s) to track"))?
        else {
            return self.cancelled();
        };
        let cmd = JjCommand::file_track(&file_path, self.global_args.clone());
        self.queue_jj_command(cmd)
    }

    pub fn jj_file_untrack(&mut self) -> Result<()> {
        let Some(file_path) = self.get_selected_file_path() else {
            return self.invalid_selection();
        };
        if !self.is_selected_working_copy() {
            return self.invalid_selection();
        }
        let cmd = JjCommand::file_untrack(file_path, self.global_args.clone());
        self.queue_jj_command(cmd)
    }

    pub fn jj_git_fetch(&mut self, mode: GitFetchMode, term: Term) -> Result<()> {
        let (flag, value) = match mode {
            GitFetchMode::Default => (None, None),
            GitFetchMode::AllRemotes => (Some("--all-remotes"), None),
            GitFetchMode::Tracked => (Some("--tracked"), None),
            GitFetchMode::Branch => {
                let Some(branch) =
                    get_input_from_editor(term, None, Some("Enter the branch to fetch"))?
                else {
                    return self.cancelled();
                };
                (Some("-b"), Some(branch))
            }
            GitFetchMode::Remote => {
                let Some(remote) =
                    get_input_from_editor(term, None, Some("Enter the remote to fetch from"))?
                else {
                    return self.cancelled();
                };
                (Some("--remote"), Some(remote))
            }
        };
        let cmd = JjCommand::git_fetch(flag, value.as_deref(), self.global_args.clone());
        self.queue_jj_command(cmd)
    }

    pub fn jj_git_push(&mut self, mode: GitPushMode, term: Term) -> Result<()> {
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
                let Some(bookmark_name) = get_input_from_editor(
                    term,
                    None,
                    Some("Enter the bookmark name for this revision"),
                )?
                else {
                    return self.cancelled();
                };
                (
                    Some("--named"),
                    Some(format!("{}={}", bookmark_name, change_id)),
                )
            }
            GitPushMode::Bookmark => {
                let Some(bookmark_name) =
                    get_input_from_editor(term, None, Some("Enter the bookmark to push"))?
                else {
                    return self.cancelled();
                };
                (Some("-b"), Some(bookmark_name))
            }
        };
        let cmd = JjCommand::git_push(flag, value.as_deref(), self.global_args.clone());
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

        let cmd = JjCommand::interdiff(from, to, maybe_file_path, self.global_args.clone(), term);
        self.queue_jj_command(cmd)
    }

    pub fn jj_metaedit(&mut self, action: MetaeditAction, term: Term) -> Result<()> {
        let Some(change_id) = self.get_selected_change_id() else {
            return self.invalid_selection();
        };

        let (flag, value) = match action {
            MetaeditAction::UpdateChangeId => ("--update-change-id", None),
            MetaeditAction::UpdateAuthorTimestamp => ("--update-author-timestamp", None),
            MetaeditAction::UpdateAuthor => ("--update-author", None),
            MetaeditAction::ForceRewrite => ("--force-rewrite", None),
            MetaeditAction::SetAuthor => {
                let Some(author) = get_input_from_editor(
                    term,
                    None,
                    Some("Enter the author (e.g. 'Name <email@example.com>')"),
                )?
                else {
                    return self.cancelled();
                };
                ("--author", Some(author))
            }
            MetaeditAction::SetAuthorTimestamp => {
                let Some(timestamp) = get_input_from_editor(
                    term,
                    None,
                    Some("Enter the author timestamp (e.g. '2000-01-23T01:23:45-08:00')"),
                )?
                else {
                    return self.cancelled();
                };
                ("--author-timestamp", Some(timestamp))
            }
        };

        let cmd = JjCommand::metaedit(change_id, flag, value.as_deref(), self.global_args.clone());
        self.queue_jj_command(cmd)
    }

    pub fn jj_new(&mut self, mode: NewMode) -> Result<()> {
        let cmd = match mode {
            NewMode::Default => {
                let Some(change_id) = self.get_selected_change_id() else {
                    return self.invalid_selection();
                };
                JjCommand::new(change_id, &[], self.global_args.clone())
            }
            NewMode::AfterTrunk => JjCommand::new("trunk()", &[], self.global_args.clone()),
            NewMode::Before => {
                let Some(change_id) = self.get_selected_change_id() else {
                    return self.invalid_selection();
                };
                JjCommand::new(
                    change_id,
                    &["--no-edit", "--insert-before"],
                    self.global_args.clone(),
                )
            }
            NewMode::InsertAfter => {
                let Some(change_id) = self.get_selected_change_id() else {
                    return self.invalid_selection();
                };
                JjCommand::new(change_id, &["--insert-after"], self.global_args.clone())
            }
        };
        self.queue_jj_command(cmd)
    }

    pub fn jj_new_after_trunk_sync(&mut self) -> Result<()> {
        let fetch_cmd = JjCommand::git_fetch(None, None, self.global_args.clone());
        let new_cmd = JjCommand::new("trunk()", &[], self.global_args.clone());
        self.queue_jj_commands(vec![fetch_cmd, new_cmd])
    }

    pub fn jj_new_at_target(&mut self, term: Term) -> Result<()> {
        let Some(target) =
            get_input_from_editor(term, None, Some("Enter the revision or bookmark"))?
        else {
            return self.cancelled();
        };
        let cmd = JjCommand::new(&target, &[], self.global_args.clone());
        self.queue_jj_command(cmd)
    }

    pub fn jj_next_prev(
        &mut self,
        direction: NextPrevDirection,
        mode: NextPrevMode,
        offset: bool,
        term: Term,
    ) -> Result<()> {
        let mode = match mode {
            NextPrevMode::Conflict => Some("--conflict"),
            NextPrevMode::Default => None,
            NextPrevMode::Edit => Some("--edit"),
            NextPrevMode::NoEdit => Some("--no-edit"),
        };

        let offset = if offset {
            let Some(offset) = get_input_from_editor(term, None, Some("Enter the offset"))? else {
                self.cancelled()?;
                return Ok(());
            };
            Some(offset)
        } else {
            None
        };

        let direction = match direction {
            NextPrevDirection::Next => "next",
            NextPrevDirection::Prev => "prev",
        };
        let cmd =
            JjCommand::next_prev(direction, mode, offset.as_deref(), self.global_args.clone());
        self.queue_jj_command(cmd)
    }

    pub fn jj_parallelize(&mut self, source: ParallelizeSource, term: Term) -> Result<()> {
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
                let Some(revset) =
                    get_input_from_editor(term, None, Some("Enter the revset to parallelize"))?
                else {
                    return self.cancelled();
                };
                revset
            }
            ParallelizeSource::Selection => {
                let Some(change_id) = self.get_selected_change_id() else {
                    return self.invalid_selection();
                };
                format!("{}-::{}", change_id, change_id)
            }
        };
        let cmd = JjCommand::parallelize(&revset, self.global_args.clone());
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

        let cmd = JjCommand::rebase(
            source_type,
            source_change_id,
            destination_type,
            destination,
            self.global_args.clone(),
        );
        self.queue_jj_command(cmd)
    }

    pub fn jj_rebase_selected_branch_onto_trunk(&mut self) -> Result<()> {
        let Some(source_change_id) = self.get_selected_change_id() else {
            return self.invalid_selection();
        };

        let cmd = JjCommand::rebase(
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

        let fetch_cmd = JjCommand::git_fetch(None, None, self.global_args.clone());
        let rebase_cmd = JjCommand::rebase(
            "--branch",
            source_change_id,
            "--onto",
            "trunk()",
            self.global_args.clone(),
        );
        self.queue_jj_commands(vec![fetch_cmd, rebase_cmd])
    }

    pub fn jj_redo(&mut self) -> Result<()> {
        let cmd = JjCommand::redo(self.global_args.clone());
        self.queue_jj_command(cmd)
    }

    pub fn jj_resolve(&mut self, term: Term) -> Result<()> {
        let Some(change_id) = self.get_selected_change_id() else {
            return self.invalid_selection();
        };
        let maybe_file_path = self.get_selected_file_path();
        let cmd = JjCommand::resolve(change_id, maybe_file_path, self.global_args.clone(), term);
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

        let cmd = JjCommand::restore(&flags, maybe_file_path, self.global_args.clone());
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

        let cmd = JjCommand::revert(
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
        let cmd = JjCommand::sign(action, &revset, self.global_args.clone());
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
        let cmd = JjCommand::simplify_parents(change_id, mode, self.global_args.clone());
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
                    JjCommand::squash_noninteractive(
                        &commit.change_id,
                        maybe_file_path,
                        self.global_args.clone(),
                    )
                } else {
                    JjCommand::squash_interactive(
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
                JjCommand::squash_into_interactive(
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
        let cmd = JjCommand::status(self.global_args.clone(), term);
        self.queue_jj_command(cmd)
    }

    pub fn jj_undo(&mut self) -> Result<()> {
        let cmd = JjCommand::undo(self.global_args.clone());
        self.queue_jj_command(cmd)
    }

    pub fn jj_view(&mut self, mode: ViewMode, term: Term) -> Result<()> {
        let cmd = match mode {
            ViewMode::Default => {
                let Some(change_id) = self.get_selected_change_id() else {
                    return self.invalid_selection();
                };
                match self.get_selected_file_path() {
                    Some(file_path) => JjCommand::diff_file_interactive(
                        change_id,
                        file_path,
                        self.global_args.clone(),
                        term,
                    ),
                    None => JjCommand::show(change_id, self.global_args.clone(), term),
                }
            }
            ViewMode::FromSelection => {
                let Some(from_change_id) = self.get_selected_change_id() else {
                    return self.invalid_selection();
                };
                let file = self.get_selected_file_path();
                JjCommand::diff_from_to_interactive(
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
                JjCommand::diff_from_to_interactive(
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
                JjCommand::diff_from_to_interactive(
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
                JjCommand::diff_from_to_interactive(
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
                    if cmd.sync() {
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
