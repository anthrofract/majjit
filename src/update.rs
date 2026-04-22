use crate::{
    model::{Model, State},
    terminal::Term,
};
use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyModifiers, MouseEventKind};
use std::time::Duration;

const EVENT_POLL_DURATION: Duration = Duration::from_millis(200);

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Message {
    Abandon {
        mode: AbandonMode,
    },
    Absorb {
        mode: AbsorbMode,
    },
    BookmarkCreate,
    BookmarkDelete,
    BookmarkForget {
        include_remotes: bool,
    },
    BookmarkListAll,
    BookmarkListLocal,
    BookmarkListTracked,
    BookmarkListUntracked,
    BookmarkListConflicted,
    BookmarkMove {
        mode: BookmarkMoveMode,
    },
    BookmarkRename,
    BookmarkSet {
        mode: BookmarkSetMode,
    },
    BookmarkTrack,
    BookmarkUntrack,
    Clear,
    Custom,
    Commit,
    Describe,
    DescribeInline,
    Duplicate {
        destination_type: DuplicateDestinationType,
        destination: DuplicateDestination,
    },
    Edit,
    EditTarget,
    Evolog {
        patch: bool,
    },
    FileTrack,
    FileUntrack,
    GitFetch {
        mode: GitFetchMode,
    },
    GitPush {
        mode: GitPushMode,
    },
    Interdiff {
        mode: InterdiffMode,
    },
    LeftMouseClick {
        row: u16,
        column: u16,
    },
    Metaedit {
        action: MetaeditAction,
    },
    New {
        mode: NewMode,
    },
    NewAfterTrunkSync,
    NewAtTarget,
    NewRevsets,
    NextPrev {
        direction: NextPrevDirection,
        mode: NextPrevMode,
        offset: bool,
    },
    Open,
    Parallelize {
        source: ParallelizeSource,
    },
    Quit,
    Rebase {
        source_type: RebaseSourceType,
        destination_type: RebaseDestinationType,
        destination: RebaseDestination,
    },
    RebaseSelectedBranchOntoTrunk,
    RebaseSelectedBranchOntoTrunkSync,
    RebaseCustom,
    RebaseTargetFuzzy {
        source_type: RebaseSourceType,
        destination_type: RebaseDestinationType,
    },
    Redo,
    Refresh,
    Resolve,
    Restore {
        mode: RestoreMode,
    },
    Revert {
        revision: RevertRevision,
        destination_type: RevertDestinationType,
        destination: RevertDestination,
    },
    RightMouseClick {
        row: u16,
        column: u16,
    },
    SaveSelection,
    ScrollDown,
    ScrollDownPage,
    ScrollUp,
    ScrollUpPage,
    SelectByBookmark,
    SelectByDescription,
    SelectCurrentWorkingCopy,
    SelectInRevset,
    SelectNextNode,
    SelectNextSiblingNode,
    SelectParentNode,
    SelectPrevNode,
    SelectPrevSiblingNode,
    SetRevset {
        mode: SetRevsetMode,
    },
    ShowHelp,
    Sign {
        action: SignAction,
        range: bool,
    },
    SimplifyParents {
        mode: SimplifyParentsMode,
    },
    Squash {
        mode: SquashMode,
    },
    Status,
    SubmitTextInput,
    ToggleIgnoreImmutable,
    ToggleLogListFold,
    Undo,
    View {
        mode: ViewMode,
    },
    WorkspaceAddPathOnly,
    WorkspaceAddNamed,
    WorkspaceForgetAtSelection,
    WorkspaceForgetCurrent,
    WorkspaceForgetFuzzy,
    WorkspaceList,
    WorkspaceRename,
    WorkspaceUpdateStale,
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum AbandonMode {
    Default,
    RetainBookmarks,
    RestoreDescendants,
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum AbsorbMode {
    Default,
    Into,
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum BookmarkMoveMode {
    AllowBackwards,
    Default,
    Tug,
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum BookmarkSetMode {
    AllowBackwards,
    Default,
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum DuplicateDestination {
    Default,
    Selection,
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum DuplicateDestinationType {
    Default,
    InsertAfter,
    InsertBefore,
    Onto,
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum GitFetchMode {
    AllRemotes,
    Branch,
    Default,
    Remote,
    Tracked,
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum GitPushMode {
    All,
    Bookmark,
    Change,
    Default,
    Deleted,
    Named,
    Revision,
    Tracked,
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum InterdiffMode {
    FromSelection,
    FromSelectionToDestination,
    ToSelection,
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum MetaeditAction {
    ForceRewrite,
    SetAuthor,
    SetAuthorTimestamp,
    UpdateAuthor,
    UpdateAuthorTimestamp,
    UpdateChangeId,
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum NewMode {
    AfterTrunk,
    Before,
    Default,
    InsertAfter,
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum NextPrevDirection {
    Next,
    Prev,
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum NextPrevMode {
    Conflict,
    Default,
    Edit,
    NoEdit,
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum ParallelizeSource {
    Range,
    Revset,
    Selection,
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum RebaseDestination {
    Current,
    Selection,
    Trunk,
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum RebaseDestinationType {
    InsertAfter,
    InsertBefore,
    Onto,
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum RebaseSourceType {
    Branch,
    Revisions,
    Source,
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum RestoreMode {
    ChangesIn,
    ChangesInRestoreDescendants,
    From,
    FromInto,
    Into,
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum RevertDestination {
    Current,
    Selection,
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum RevertDestinationType {
    InsertAfter,
    InsertBefore,
    Onto,
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum RevertRevision {
    Saved,
    Selection,
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum SetRevsetMode {
    All,
    Bookmarks,
    Conflicts,
    Custom,
    Default,
    JjDefault,
    Mine,
    Mutable,
    Recent,
    Stack,
    WorkingCopyAncestry,
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum SignAction {
    Sign,
    Unsign,
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum SimplifyParentsMode {
    Revisions,
    Source,
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum SquashMode {
    Default,
    Into,
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum ViewMode {
    Default,
    FromSelection,
    FromSelectionToDestination,
    FromTrunkToSelection,
    ToSelection,
}

pub fn update(terminal: Term, model: &mut Model) -> Result<()> {
    model.process_jj_command_queue()?;

    let mut current_msg = handle_event(model)?;
    while let Some(msg) = current_msg {
        current_msg = handle_msg(terminal.clone(), model, msg)?;
    }

    Ok(())
}

fn handle_event(model: &mut Model) -> Result<Option<Message>> {
    if event::poll(EVENT_POLL_DURATION)? {
        match event::read()? {
            Event::Key(key) if key.kind == event::KeyEventKind::Press => {
                return Ok(handle_key(model, key));
            }
            Event::Mouse(mouse) => {
                return Ok(handle_mouse(mouse));
            }
            _ => {}
        }
    }
    Ok(None)
}

fn handle_key(model: &mut Model, key: event::KeyEvent) -> Option<Message> {
    if model.state == State::EnteringText {
        if model.has_active_fuzzy() {
            return match key.code {
                KeyCode::Esc => Some(Message::Clear),
                KeyCode::Enter => Some(Message::SubmitTextInput),
                KeyCode::Up | KeyCode::Tab => {
                    model.move_fuzzy_selection_up();
                    None
                }
                KeyCode::Down | KeyCode::BackTab => {
                    model.move_fuzzy_selection_down();
                    None
                }
                KeyCode::PageUp => {
                    model.page_fuzzy_selection_up();
                    None
                }
                KeyCode::PageDown => {
                    model.page_fuzzy_selection_down();
                    None
                }
                _ => {
                    model.forward_text_input_key(key);
                    model.update_fuzzy_filter();
                    None
                }
            };
        }
        return match key.code {
            KeyCode::Esc => Some(Message::Clear),
            KeyCode::Enter => Some(Message::SubmitTextInput),
            _ => {
                model.forward_text_input_key(key);
                None
            }
        };
    }

    match key.code {
        KeyCode::Char('q') => Some(Message::Quit),
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => Some(Message::Quit),
        KeyCode::Down | KeyCode::Char('j') => Some(Message::SelectNextNode),
        KeyCode::Up | KeyCode::Char('k') => Some(Message::SelectPrevNode),
        KeyCode::PageDown => Some(Message::ScrollDownPage),
        KeyCode::PageUp => Some(Message::ScrollUpPage),
        KeyCode::Left | KeyCode::Char('h') => Some(Message::SelectPrevSiblingNode),
        KeyCode::Right | KeyCode::Char('l') => Some(Message::SelectNextSiblingNode),
        KeyCode::Char('K') => Some(Message::SelectParentNode),
        KeyCode::Char(' ') | KeyCode::Backspace => Some(Message::Refresh),
        KeyCode::Tab => Some(Message::ToggleLogListFold),
        KeyCode::Esc => Some(Message::Clear),
        KeyCode::Char('@') => Some(Message::SelectCurrentWorkingCopy),
        KeyCode::Char('I') => Some(Message::ToggleIgnoreImmutable),
        KeyCode::Char('?') => Some(Message::ShowHelp),
        _ => model.handle_command_key(key.code),
    }
}

fn handle_mouse(mouse: event::MouseEvent) -> Option<Message> {
    match mouse.kind {
        MouseEventKind::ScrollDown => Some(Message::ScrollDown),
        MouseEventKind::ScrollUp => Some(Message::ScrollUp),
        MouseEventKind::Down(event::MouseButton::Left) => Some(Message::LeftMouseClick {
            row: mouse.row,
            column: mouse.column,
        }),
        MouseEventKind::Down(event::MouseButton::Right) => Some(Message::RightMouseClick {
            row: mouse.row,
            column: mouse.column,
        }),
        _ => None,
    }
}

fn handle_msg(term: Term, model: &mut Model, msg: Message) -> Result<Option<Message>> {
    match msg {
        // General
        Message::Clear => model.clear(),
        Message::Quit => model.quit(),
        Message::Refresh => model.refresh()?,
        Message::SetRevset { mode } => model.set_revset(mode),
        Message::SubmitTextInput => return model.submit_text_input(),
        Message::ShowHelp => model.show_help(),
        Message::ToggleIgnoreImmutable => model.toggle_ignore_immutable(),

        // Navigation
        Message::ScrollDownPage => model.scroll_down_page(),
        Message::ScrollUpPage => model.scroll_up_page(),
        Message::SelectByBookmark => model.select_by_bookmark(),
        Message::SelectByDescription => model.select_by_description(),
        Message::SelectCurrentWorkingCopy => model.select_current_working_copy(),
        Message::SelectInRevset => model.select_in_revset(),
        Message::SelectNextNode => model.select_next_node(),
        Message::SelectNextSiblingNode => model.select_current_next_sibling_node()?,
        Message::SelectParentNode => model.select_parent_node()?,
        Message::SelectPrevNode => model.select_prev_node(),
        Message::SelectPrevSiblingNode => model.select_current_prev_sibling_node()?,
        Message::ToggleLogListFold => model.toggle_current_fold()?,

        // Mouse
        Message::LeftMouseClick { row, column } => model.handle_mouse_click(row, column),
        Message::RightMouseClick { row, column } => {
            model.handle_mouse_click(row, column);
            model.toggle_current_fold()?;
        }
        Message::ScrollDown => model.scroll_down_once(),
        Message::ScrollUp => model.scroll_up_once(),

        // Commands
        Message::Abandon { mode } => model.jj_abandon(mode)?,
        Message::Absorb { mode } => model.jj_absorb(mode)?,
        Message::BookmarkCreate => model.jj_bookmark_create()?,
        Message::BookmarkDelete => model.jj_bookmark_delete()?,
        Message::BookmarkForget { include_remotes } => model.jj_bookmark_forget(include_remotes)?,
        Message::BookmarkListAll => model.jj_bookmark_list_all()?,
        Message::BookmarkListLocal => model.jj_bookmark_list_local()?,
        Message::BookmarkListTracked => model.jj_bookmark_list_tracked()?,
        Message::BookmarkListUntracked => model.jj_bookmark_list_untracked()?,
        Message::BookmarkListConflicted => model.jj_bookmark_list_conflicted()?,
        Message::BookmarkMove { mode } => model.jj_bookmark_move(mode)?,
        Message::BookmarkRename => model.jj_bookmark_rename()?,
        Message::BookmarkSet { mode } => model.jj_bookmark_set(mode)?,
        Message::BookmarkTrack => model.jj_bookmark_track()?,
        Message::BookmarkUntrack => model.jj_bookmark_untrack()?,
        Message::Commit => model.jj_commit(term)?,
        Message::Custom => model.jj_custom()?,
        Message::Describe => model.jj_describe(term)?,
        Message::DescribeInline => model.start_describe_input()?,
        Message::Duplicate {
            destination_type,
            destination,
        } => model.jj_duplicate(destination_type, destination)?,
        Message::Edit => model.jj_edit()?,
        Message::EditTarget => model.jj_edit_target()?,
        Message::Evolog { patch } => model.jj_evolog(patch, term)?,
        Message::FileTrack => model.jj_file_track()?,
        Message::FileUntrack => model.jj_file_untrack()?,
        Message::GitFetch { mode } => model.jj_git_fetch(mode)?,
        Message::GitPush { mode } => model.jj_git_push(mode)?,
        Message::Interdiff { mode } => model.jj_interdiff(mode, term)?,
        Message::Metaedit { action } => model.jj_metaedit(action)?,
        Message::New { mode } => model.jj_new(mode)?,
        Message::NewAfterTrunkSync => model.jj_new_after_trunk_sync()?,
        Message::NewAtTarget => model.jj_new_at_target()?,
        Message::NewRevsets => model.jj_new_revsets()?,
        Message::NextPrev {
            direction,
            mode,
            offset,
        } => model.jj_next_prev(direction, mode, offset)?,
        Message::Open => model.open_file(term)?,
        Message::Parallelize { source } => model.jj_parallelize(source)?,
        Message::Rebase {
            source_type,
            destination_type,
            destination,
        } => model.jj_rebase(source_type, destination_type, destination)?,
        Message::RebaseSelectedBranchOntoTrunk => model.jj_rebase_selected_branch_onto_trunk()?,
        Message::RebaseSelectedBranchOntoTrunkSync => {
            model.jj_rebase_selected_branch_onto_trunk_sync()?
        }
        Message::RebaseCustom => model.jj_rebase_custom()?,
        Message::RebaseTargetFuzzy {
            source_type,
            destination_type,
        } => model.jj_rebase_target_fuzzy(source_type, destination_type)?,
        Message::Redo => model.jj_redo()?,
        Message::Resolve => model.jj_resolve(term)?,
        Message::Restore { mode } => model.jj_restore(mode)?,
        Message::Revert {
            revision,
            destination_type,
            destination,
        } => model.jj_revert(revision, destination_type, destination)?,
        Message::SaveSelection => model.save_selection()?,
        Message::Sign { action, range } => model.jj_sign(action, range)?,
        Message::SimplifyParents { mode } => model.jj_simplify_parents(mode)?,
        Message::Squash { mode } => model.jj_squash(mode, term)?,
        Message::Status => model.jj_status(term)?,
        Message::Undo => model.jj_undo()?,
        Message::View { mode } => model.jj_view(mode, term)?,
        Message::WorkspaceAddPathOnly => model.jj_workspace_add_path_only()?,
        Message::WorkspaceAddNamed => model.jj_workspace_add_named()?,
        Message::WorkspaceForgetAtSelection => model.jj_workspace_forget_at_selection()?,
        Message::WorkspaceForgetCurrent => model.jj_workspace_forget_current()?,
        Message::WorkspaceForgetFuzzy => model.jj_workspace_forget_fuzzy()?,
        Message::WorkspaceList => model.jj_workspace_list()?,
        Message::WorkspaceRename => model.jj_workspace_rename()?,
        Message::WorkspaceUpdateStale => model.jj_workspace_update_stale()?,
    };

    Ok(None)
}
