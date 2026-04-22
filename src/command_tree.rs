use crate::update::{
    AbandonMode, AbsorbMode, BookmarkMoveMode, BookmarkSetMode, DuplicateDestination,
    DuplicateDestinationType, GitFetchMode, GitPushMode, InterdiffMode, Message, MetaeditAction,
    NewMode, NextPrevDirection, NextPrevMode, ParallelizeSource, RebaseDestination,
    RebaseDestinationType, RebaseSourceType, RestoreMode, RevertDestination, RevertDestinationType,
    RevertRevision, SetRevsetMode, SignAction, SimplifyParentsMode, SquashMode, ViewMode,
};
use crossterm::event::KeyCode;
use indexmap::IndexMap;
use ratatui::{
    style::{Color, Style},
    text::{Line, Span, Text},
};
use std::collections::HashMap;

type HelpEntries = IndexMap<String, Vec<(String, String)>>;

#[derive(Debug, Clone)]
pub struct CommandTreeNodeChildren {
    nodes: HashMap<KeyCode, CommandTreeNode>,
    help: HelpEntries,
}

impl CommandTreeNodeChildren {
    fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            help: IndexMap::new(),
        }
    }

    pub fn get_node(&self, key_code: &KeyCode) -> Option<&CommandTreeNode> {
        self.nodes.get(key_code)
    }

    fn get_node_mut(&mut self, key_code: &KeyCode) -> Option<&mut CommandTreeNode> {
        self.nodes.get_mut(key_code)
    }

    fn get_help_entries(&self) -> HelpEntries {
        let mut help = self.help.clone();

        for (_, entries) in help.iter_mut() {
            entries.sort_by_key(|a| a.0.to_lowercase());
        }

        help
    }

    pub fn get_help(&self) -> Text<'static> {
        let entries = self.get_help_entries();
        render_help_text(entries)
    }

    fn add_child(
        &mut self,
        help_group_text: &str,
        help_text: &str,
        key_code: KeyCode,
        node: CommandTreeNode,
    ) {
        self.nodes.insert(key_code, node);
        let help_group = self.help.entry(help_group_text.to_string()).or_default();
        help_group.push((key_code.to_string(), help_text.to_string()))
    }
}

#[derive(Debug, Clone)]
pub struct CommandTreeNode {
    pub children: Option<CommandTreeNodeChildren>,
    pub action: Option<Message>,
}

impl CommandTreeNode {
    pub fn new_children() -> Self {
        Self {
            children: Some(CommandTreeNodeChildren::new()),
            action: None,
        }
    }

    pub fn new_action(action: Message) -> Self {
        Self {
            children: None,
            action: Some(action),
        }
    }

    pub fn new_action_with_children(action: Message) -> Self {
        let mut node = Self::new_children();
        node.action = Some(action);
        node
    }
}

#[derive(Debug)]
pub struct CommandTree(CommandTreeNode);

impl CommandTree {
    fn add_children(&mut self, entries: Vec<(&str, &str, Vec<KeyCode>, CommandTreeNode)>) {
        for (help_group_text, help_text, key_codes, node) in entries {
            let (last_key, rest_keys) = key_codes.split_last().unwrap();
            let dest_node = self.get_node_mut(rest_keys).unwrap();
            let children = dest_node.children.as_mut().unwrap();
            children.add_child(help_group_text, help_text, *last_key, node)
        }
    }

    pub fn get_node(&self, key_codes: &[KeyCode]) -> Option<&CommandTreeNode> {
        let mut node = &self.0;

        for key_code in key_codes {
            let children = match &node.children {
                None => return None,
                Some(children) => children,
            };
            node = children.get_node(key_code)?;
        }

        Some(node)
    }

    fn get_node_mut(&mut self, key_codes: &[KeyCode]) -> Option<&mut CommandTreeNode> {
        let mut node = &mut self.0;

        for key_code in key_codes {
            let children = match &mut node.children {
                None => return None,
                Some(children) => children,
            };
            node = children.get_node_mut(key_code)?;
        }

        Some(node)
    }

    pub fn get_help(&self) -> Text<'static> {
        let nav_help = [
            ("Tab ", "Toggle folding"),
            ("PgDn", "Move down page"),
            ("PgUp", "Move up page"),
            ("j/↓ ", "Move down"),
            ("k/↑ ", "Move up"),
            ("l/→ ", "Next sibling"),
            ("h/← ", "Prev sibling"),
            ("K", "Select parent"),
            ("@", "Select @ change"),
        ]
        .iter()
        .map(|(key, help)| (key.to_string(), help.to_string()))
        .collect();

        let general_help = [
            ("Spc/Bksp", "Refresh log tree"),
            ("Esc", "Clear app state"),
            ("I", "Toggle --ignore-immutable"),
            ("?", "Show help"),
            ("q", "Quit"),
        ]
        .iter()
        .map(|(key, help)| (key.to_string(), help.to_string()))
        .collect();

        let mut entries = self.0.children.as_ref().unwrap().get_help_entries();
        entries.insert("Navigation".to_string(), nav_help);
        entries.insert("General".to_string(), general_help);
        render_help_text(entries)
    }

    pub fn new() -> Self {
        let items = vec![
            (
                "Commands",
                "Abandon",
                vec![KeyCode::Char('a')],
                CommandTreeNode::new_children(),
            ),
            (
                "Abandon",
                "Selection",
                vec![KeyCode::Char('a'), KeyCode::Char('a')],
                CommandTreeNode::new_action(Message::Abandon {
                    mode: AbandonMode::Default,
                }),
            ),
            (
                "Abandon",
                "Selection (retain bookmarks)",
                vec![KeyCode::Char('a'), KeyCode::Char('b')],
                CommandTreeNode::new_action(Message::Abandon {
                    mode: AbandonMode::RetainBookmarks,
                }),
            ),
            (
                "Abandon",
                "Selection (restore descendants)",
                vec![KeyCode::Char('a'), KeyCode::Char('d')],
                CommandTreeNode::new_action(Message::Abandon {
                    mode: AbandonMode::RestoreDescendants,
                }),
            ),
            (
                "Commands",
                "Absorb",
                vec![KeyCode::Char('A')],
                CommandTreeNode::new_children(),
            ),
            (
                "Absorb",
                "From selection",
                vec![KeyCode::Char('A'), KeyCode::Char('a')],
                CommandTreeNode::new_action(Message::Absorb {
                    mode: AbsorbMode::Default,
                }),
            ),
            (
                "Absorb",
                "From selection into destination",
                vec![KeyCode::Char('A'), KeyCode::Char('i')],
                CommandTreeNode::new_action_with_children(Message::SaveSelection),
            ),
            (
                "Absorb into",
                "Select destination",
                vec![KeyCode::Char('A'), KeyCode::Char('i'), KeyCode::Enter],
                CommandTreeNode::new_action(Message::Absorb {
                    mode: AbsorbMode::Into,
                }),
            ),
            (
                "Commands",
                "Bookmark",
                vec![KeyCode::Char('b')],
                CommandTreeNode::new_children(),
            ),
            (
                "Commands",
                "Custom",
                vec![KeyCode::Char('C')],
                CommandTreeNode::new_action(Message::Custom),
            ),
            (
                "Bookmark",
                "Create at selection",
                vec![KeyCode::Char('b'), KeyCode::Char('c')],
                CommandTreeNode::new_action(Message::BookmarkCreate),
            ),
            (
                "Bookmark",
                "List",
                vec![KeyCode::Char('b'), KeyCode::Char('L')],
                CommandTreeNode::new_children(),
            ),
            (
                "Bookmark list",
                "All",
                vec![KeyCode::Char('b'), KeyCode::Char('L'), KeyCode::Char('a')],
                CommandTreeNode::new_action(Message::BookmarkListAll),
            ),
            (
                "Bookmark list",
                "Local only",
                vec![KeyCode::Char('b'), KeyCode::Char('L'), KeyCode::Char('L')],
                CommandTreeNode::new_action(Message::BookmarkListLocal),
            ),
            (
                "Bookmark list",
                "Tracked remote",
                vec![KeyCode::Char('b'), KeyCode::Char('L'), KeyCode::Char('t')],
                CommandTreeNode::new_action(Message::BookmarkListTracked),
            ),
            (
                "Bookmark list",
                "Untracked remote",
                vec![KeyCode::Char('b'), KeyCode::Char('L'), KeyCode::Char('u')],
                CommandTreeNode::new_action(Message::BookmarkListUntracked),
            ),
            (
                "Bookmark list",
                "Conflicted",
                vec![KeyCode::Char('b'), KeyCode::Char('L'), KeyCode::Char('c')],
                CommandTreeNode::new_action(Message::BookmarkListConflicted),
            ),
            (
                "Bookmark",
                "Move",
                vec![KeyCode::Char('b'), KeyCode::Char('m')],
                CommandTreeNode::new_children(),
            ),
            (
                "Bookmark move",
                "Selected bookmark to destination",
                vec![KeyCode::Char('b'), KeyCode::Char('m'), KeyCode::Char('m')],
                CommandTreeNode::new_action_with_children(Message::SaveSelection),
            ),
            (
                "Move bookmark to",
                "Select destination",
                vec![
                    KeyCode::Char('b'),
                    KeyCode::Char('m'),
                    KeyCode::Char('m'),
                    KeyCode::Enter,
                ],
                CommandTreeNode::new_action(Message::BookmarkMove {
                    mode: BookmarkMoveMode::Default,
                }),
            ),
            (
                "Bookmark move",
                "Selected bookmark to destination (allow backwards)",
                vec![KeyCode::Char('b'), KeyCode::Char('m'), KeyCode::Char('M')],
                CommandTreeNode::new_action_with_children(Message::SaveSelection),
            ),
            (
                "Move bookmark to (allow backwards)",
                "Select destination",
                vec![
                    KeyCode::Char('b'),
                    KeyCode::Char('m'),
                    KeyCode::Char('M'),
                    KeyCode::Enter,
                ],
                CommandTreeNode::new_action(Message::BookmarkMove {
                    mode: BookmarkMoveMode::AllowBackwards,
                }),
            ),
            (
                "Bookmark move",
                "Tug to selection",
                vec![KeyCode::Char('b'), KeyCode::Char('m'), KeyCode::Char('t')],
                CommandTreeNode::new_action(Message::BookmarkMove {
                    mode: BookmarkMoveMode::Tug,
                }),
            ),
            (
                "Bookmark",
                "Rename",
                vec![KeyCode::Char('b'), KeyCode::Char('r')],
                CommandTreeNode::new_action(Message::BookmarkRename),
            ),
            (
                "Bookmark",
                "Track",
                vec![KeyCode::Char('b'), KeyCode::Char('t')],
                CommandTreeNode::new_action(Message::BookmarkTrack),
            ),
            (
                "Bookmark",
                "Untrack",
                vec![KeyCode::Char('b'), KeyCode::Char('u')],
                CommandTreeNode::new_action(Message::BookmarkUntrack),
            ),
            (
                "Bookmark",
                "Delete",
                vec![KeyCode::Char('b'), KeyCode::Char('d')],
                CommandTreeNode::new_action(Message::BookmarkDelete),
            ),
            (
                "Bookmark",
                "Forget",
                vec![KeyCode::Char('b'), KeyCode::Char('f')],
                CommandTreeNode::new_action(Message::BookmarkForget {
                    include_remotes: false,
                }),
            ),
            (
                "Bookmark",
                "Forget (including remotes)",
                vec![KeyCode::Char('b'), KeyCode::Char('F')],
                CommandTreeNode::new_action(Message::BookmarkForget {
                    include_remotes: true,
                }),
            ),
            (
                "Bookmark",
                "Set to selection",
                vec![KeyCode::Char('b'), KeyCode::Char('s')],
                CommandTreeNode::new_action(Message::BookmarkSet {
                    mode: BookmarkSetMode::Default,
                }),
            ),
            (
                "Bookmark",
                "Set to selection (allow backwards)",
                vec![KeyCode::Char('b'), KeyCode::Char('S')],
                CommandTreeNode::new_action(Message::BookmarkSet {
                    mode: BookmarkSetMode::AllowBackwards,
                }),
            ),
            (
                "Commands",
                "Commit",
                vec![KeyCode::Char('c')],
                CommandTreeNode::new_children(),
            ),
            (
                "Commit",
                "Selection",
                vec![KeyCode::Char('c'), KeyCode::Char('c')],
                CommandTreeNode::new_action(Message::Commit),
            ),
            (
                "Commands",
                "Describe",
                vec![KeyCode::Char('d')],
                CommandTreeNode::new_children(),
            ),
            (
                "Describe",
                "Selection",
                vec![KeyCode::Char('d'), KeyCode::Char('d')],
                CommandTreeNode::new_action(Message::DescribeInline),
            ),
            (
                "Describe",
                "Selection in editor",
                vec![KeyCode::Char('d'), KeyCode::Char('D')],
                CommandTreeNode::new_action(Message::Describe),
            ),
            (
                "Commands",
                "Duplicate",
                vec![KeyCode::Char('D')],
                CommandTreeNode::new_children(),
            ),
            (
                "Duplicate",
                "Selection",
                vec![KeyCode::Char('D'), KeyCode::Char('d')],
                CommandTreeNode::new_action(Message::Duplicate {
                    destination_type: DuplicateDestinationType::Default,
                    destination: DuplicateDestination::Default,
                }),
            ),
            (
                "Commands",
                "Select",
                vec![KeyCode::Char('/')],
                CommandTreeNode::new_children(),
            ),
            (
                "Select",
                "Target",
                vec![KeyCode::Char('/'), KeyCode::Char('/')],
                CommandTreeNode::new_action(Message::SelectInRevset),
            ),
            (
                "Select",
                "Bookmark",
                vec![KeyCode::Char('/'), KeyCode::Char('b')],
                CommandTreeNode::new_action(Message::SelectByBookmark),
            ),
            (
                "Select",
                "Description",
                vec![KeyCode::Char('/'), KeyCode::Char('d')],
                CommandTreeNode::new_action(Message::SelectByDescription),
            ),
            (
                "Duplicate",
                "Selection onto destination",
                vec![KeyCode::Char('D'), KeyCode::Char('o')],
                CommandTreeNode::new_action_with_children(Message::SaveSelection),
            ),
            (
                "Duplicate onto",
                "Select destination",
                vec![KeyCode::Char('D'), KeyCode::Char('o'), KeyCode::Enter],
                CommandTreeNode::new_action(Message::Duplicate {
                    destination_type: DuplicateDestinationType::Onto,
                    destination: DuplicateDestination::Selection,
                }),
            ),
            (
                "Duplicate",
                "Selection insert after destination",
                vec![KeyCode::Char('D'), KeyCode::Char('a')],
                CommandTreeNode::new_action_with_children(Message::SaveSelection),
            ),
            (
                "Duplicate insert after",
                "Select destination",
                vec![KeyCode::Char('D'), KeyCode::Char('a'), KeyCode::Enter],
                CommandTreeNode::new_action(Message::Duplicate {
                    destination_type: DuplicateDestinationType::InsertAfter,
                    destination: DuplicateDestination::Selection,
                }),
            ),
            (
                "Duplicate",
                "Selection insert before destination",
                vec![KeyCode::Char('D'), KeyCode::Char('b')],
                CommandTreeNode::new_action_with_children(Message::SaveSelection),
            ),
            (
                "Duplicate insert before",
                "Select destination",
                vec![KeyCode::Char('D'), KeyCode::Char('b'), KeyCode::Enter],
                CommandTreeNode::new_action(Message::Duplicate {
                    destination_type: DuplicateDestinationType::InsertBefore,
                    destination: DuplicateDestination::Selection,
                }),
            ),
            (
                "Commands",
                "Edit",
                vec![KeyCode::Char('e')],
                CommandTreeNode::new_children(),
            ),
            (
                "Edit",
                "Selection",
                vec![KeyCode::Char('e'), KeyCode::Char('e')],
                CommandTreeNode::new_action(Message::Edit),
            ),
            (
                "Edit",
                "Target",
                vec![KeyCode::Char('e'), KeyCode::Char('/')],
                CommandTreeNode::new_action(Message::EditTarget),
            ),
            (
                "Commands",
                "Evolog",
                vec![KeyCode::Char('E')],
                CommandTreeNode::new_children(),
            ),
            (
                "Evolog",
                "Selection",
                vec![KeyCode::Char('E'), KeyCode::Char('e')],
                CommandTreeNode::new_action(Message::Evolog { patch: false }),
            ),
            (
                "Evolog",
                "Selection (patch)",
                vec![KeyCode::Char('E'), KeyCode::Char('E')],
                CommandTreeNode::new_action(Message::Evolog { patch: true }),
            ),
            (
                "Commands",
                "File",
                vec![KeyCode::Char('f')],
                CommandTreeNode::new_children(),
            ),
            (
                "File",
                "Track (enter filepath)",
                vec![KeyCode::Char('f'), KeyCode::Char('t')],
                CommandTreeNode::new_action(Message::FileTrack),
            ),
            (
                "File",
                "Untrack selection (must be ignored)",
                vec![KeyCode::Char('f'), KeyCode::Char('u')],
                CommandTreeNode::new_action(Message::FileUntrack),
            ),
            (
                "Commands",
                "Git",
                vec![KeyCode::Char('g')],
                CommandTreeNode::new_children(),
            ),
            (
                "Git",
                "Fetch",
                vec![KeyCode::Char('g'), KeyCode::Char('f')],
                CommandTreeNode::new_children(),
            ),
            (
                "Git fetch",
                "Default",
                vec![KeyCode::Char('g'), KeyCode::Char('f'), KeyCode::Char('f')],
                CommandTreeNode::new_action(Message::GitFetch {
                    mode: GitFetchMode::Default,
                }),
            ),
            (
                "Git fetch",
                "All remotes",
                vec![KeyCode::Char('g'), KeyCode::Char('f'), KeyCode::Char('a')],
                CommandTreeNode::new_action(Message::GitFetch {
                    mode: GitFetchMode::AllRemotes,
                }),
            ),
            (
                "Git fetch",
                "Tracked bookmarks",
                vec![KeyCode::Char('g'), KeyCode::Char('f'), KeyCode::Char('t')],
                CommandTreeNode::new_action(Message::GitFetch {
                    mode: GitFetchMode::Tracked,
                }),
            ),
            (
                "Git fetch",
                "Branch by name",
                vec![KeyCode::Char('g'), KeyCode::Char('f'), KeyCode::Char('b')],
                CommandTreeNode::new_action(Message::GitFetch {
                    mode: GitFetchMode::Branch,
                }),
            ),
            (
                "Git fetch",
                "Remote by name",
                vec![KeyCode::Char('g'), KeyCode::Char('f'), KeyCode::Char('r')],
                CommandTreeNode::new_action(Message::GitFetch {
                    mode: GitFetchMode::Remote,
                }),
            ),
            (
                "Git",
                "Push",
                vec![KeyCode::Char('g'), KeyCode::Char('p')],
                CommandTreeNode::new_children(),
            ),
            (
                "Git push",
                "Default",
                vec![KeyCode::Char('g'), KeyCode::Char('p'), KeyCode::Char('p')],
                CommandTreeNode::new_action(Message::GitPush {
                    mode: GitPushMode::Default,
                }),
            ),
            (
                "Git push",
                "All bookmarks",
                vec![KeyCode::Char('g'), KeyCode::Char('p'), KeyCode::Char('a')],
                CommandTreeNode::new_action(Message::GitPush {
                    mode: GitPushMode::All,
                }),
            ),
            (
                "Git push",
                "Bookmarks at selection",
                vec![KeyCode::Char('g'), KeyCode::Char('p'), KeyCode::Char('r')],
                CommandTreeNode::new_action(Message::GitPush {
                    mode: GitPushMode::Revision,
                }),
            ),
            (
                "Git push",
                "Tracked bookmarks",
                vec![KeyCode::Char('g'), KeyCode::Char('p'), KeyCode::Char('t')],
                CommandTreeNode::new_action(Message::GitPush {
                    mode: GitPushMode::Tracked,
                }),
            ),
            (
                "Git push",
                "Deleted bookmarks",
                vec![KeyCode::Char('g'), KeyCode::Char('p'), KeyCode::Char('d')],
                CommandTreeNode::new_action(Message::GitPush {
                    mode: GitPushMode::Deleted,
                }),
            ),
            (
                "Git push",
                "New bookmark for selection",
                vec![KeyCode::Char('g'), KeyCode::Char('p'), KeyCode::Char('c')],
                CommandTreeNode::new_action(Message::GitPush {
                    mode: GitPushMode::Change,
                }),
            ),
            (
                "Git push",
                "New named bookmark for selection",
                vec![KeyCode::Char('g'), KeyCode::Char('p'), KeyCode::Char('n')],
                CommandTreeNode::new_action(Message::GitPush {
                    mode: GitPushMode::Named,
                }),
            ),
            (
                "Git push",
                "Bookmark by name",
                vec![KeyCode::Char('g'), KeyCode::Char('p'), KeyCode::Char('b')],
                CommandTreeNode::new_action(Message::GitPush {
                    mode: GitPushMode::Bookmark,
                }),
            ),
            (
                "Commands",
                "Interdiff",
                vec![KeyCode::Char('i')],
                CommandTreeNode::new_children(),
            ),
            (
                "Interdiff",
                "From @ to selection",
                vec![KeyCode::Char('i'), KeyCode::Char('t')],
                CommandTreeNode::new_action(Message::Interdiff {
                    mode: InterdiffMode::ToSelection,
                }),
            ),
            (
                "Interdiff",
                "From selection to @",
                vec![KeyCode::Char('i'), KeyCode::Char('f')],
                CommandTreeNode::new_action(Message::Interdiff {
                    mode: InterdiffMode::FromSelection,
                }),
            ),
            (
                "Interdiff",
                "From selection to destination",
                vec![KeyCode::Char('i'), KeyCode::Char('i')],
                CommandTreeNode::new_action_with_children(Message::SaveSelection),
            ),
            (
                "Interdiff to destination",
                "Select destination",
                vec![KeyCode::Char('i'), KeyCode::Char('i'), KeyCode::Enter],
                CommandTreeNode::new_action(Message::Interdiff {
                    mode: InterdiffMode::FromSelectionToDestination,
                }),
            ),
            (
                "Commands",
                "Metaedit",
                vec![KeyCode::Char('m')],
                CommandTreeNode::new_children(),
            ),
            (
                "Metaedit",
                "Update change-id",
                vec![KeyCode::Char('m'), KeyCode::Char('c')],
                CommandTreeNode::new_action(Message::Metaedit {
                    action: MetaeditAction::UpdateChangeId,
                }),
            ),
            (
                "Metaedit",
                "Update author timestamp to now",
                vec![KeyCode::Char('m'), KeyCode::Char('t')],
                CommandTreeNode::new_action(Message::Metaedit {
                    action: MetaeditAction::UpdateAuthorTimestamp,
                }),
            ),
            (
                "Metaedit",
                "Update author to configured user",
                vec![KeyCode::Char('m'), KeyCode::Char('a')],
                CommandTreeNode::new_action(Message::Metaedit {
                    action: MetaeditAction::UpdateAuthor,
                }),
            ),
            (
                "Metaedit",
                "Set author",
                vec![KeyCode::Char('m'), KeyCode::Char('A')],
                CommandTreeNode::new_action(Message::Metaedit {
                    action: MetaeditAction::SetAuthor,
                }),
            ),
            (
                "Metaedit",
                "Set author timestamp",
                vec![KeyCode::Char('m'), KeyCode::Char('T')],
                CommandTreeNode::new_action(Message::Metaedit {
                    action: MetaeditAction::SetAuthorTimestamp,
                }),
            ),
            (
                "Metaedit",
                "Force rewrite",
                vec![KeyCode::Char('m'), KeyCode::Char('r')],
                CommandTreeNode::new_action(Message::Metaedit {
                    action: MetaeditAction::ForceRewrite,
                }),
            ),
            (
                "Commands",
                "Log revset",
                vec![KeyCode::Char('L')],
                CommandTreeNode::new_children(),
            ),
            (
                "Log revset",
                "Default",
                vec![KeyCode::Char('L'), KeyCode::Char('d')],
                CommandTreeNode::new_action(Message::SetRevset {
                    mode: SetRevsetMode::Default,
                }),
            ),
            (
                "Log revset",
                "Jj default",
                vec![KeyCode::Char('L'), KeyCode::Char('D')],
                CommandTreeNode::new_action(Message::SetRevset {
                    mode: SetRevsetMode::JjDefault,
                }),
            ),
            (
                "Log revset",
                "Custom",
                vec![KeyCode::Char('L'), KeyCode::Char('L')],
                CommandTreeNode::new_action(Message::SetRevset {
                    mode: SetRevsetMode::Custom,
                }),
            ),
            (
                "Log revset",
                "All commits",
                vec![KeyCode::Char('L'), KeyCode::Char('a')],
                CommandTreeNode::new_action(Message::SetRevset {
                    mode: SetRevsetMode::All,
                }),
            ),
            (
                "Log revset",
                "Mutable",
                vec![KeyCode::Char('L'), KeyCode::Char('m')],
                CommandTreeNode::new_action(Message::SetRevset {
                    mode: SetRevsetMode::Mutable,
                }),
            ),
            (
                "Log revset",
                "Current stack",
                vec![KeyCode::Char('L'), KeyCode::Char('s')],
                CommandTreeNode::new_action(Message::SetRevset {
                    mode: SetRevsetMode::Stack,
                }),
            ),
            (
                "Log revset",
                "Conflicts",
                vec![KeyCode::Char('L'), KeyCode::Char('c')],
                CommandTreeNode::new_action(Message::SetRevset {
                    mode: SetRevsetMode::Conflicts,
                }),
            ),
            (
                "Log revset",
                "@ ancestry",
                vec![KeyCode::Char('L'), KeyCode::Char('w')],
                CommandTreeNode::new_action(Message::SetRevset {
                    mode: SetRevsetMode::WorkingCopyAncestry,
                }),
            ),
            (
                "Log revset",
                "Mine",
                vec![KeyCode::Char('L'), KeyCode::Char('i')],
                CommandTreeNode::new_action(Message::SetRevset {
                    mode: SetRevsetMode::Mine,
                }),
            ),
            (
                "Log revset",
                "Bookmarks and tags",
                vec![KeyCode::Char('L'), KeyCode::Char('b')],
                CommandTreeNode::new_action(Message::SetRevset {
                    mode: SetRevsetMode::Bookmarks,
                }),
            ),
            (
                "Log revset",
                "Recent",
                vec![KeyCode::Char('L'), KeyCode::Char('r')],
                CommandTreeNode::new_action(Message::SetRevset {
                    mode: SetRevsetMode::Recent,
                }),
            ),
            (
                "Commands",
                "New",
                vec![KeyCode::Char('n')],
                CommandTreeNode::new_children(),
            ),
            (
                "New",
                "After selection",
                vec![KeyCode::Char('n'), KeyCode::Char('n')],
                CommandTreeNode::new_action(Message::New {
                    mode: NewMode::Default,
                }),
            ),
            (
                "New",
                "After selection (rebase children)",
                vec![KeyCode::Char('n'), KeyCode::Char('a')],
                CommandTreeNode::new_action(Message::New {
                    mode: NewMode::InsertAfter,
                }),
            ),
            (
                "New",
                "Before selection (rebase children)",
                vec![KeyCode::Char('n'), KeyCode::Char('b')],
                CommandTreeNode::new_action(Message::New {
                    mode: NewMode::Before,
                }),
            ),
            (
                "New",
                "After trunk",
                vec![KeyCode::Char('n'), KeyCode::Char('m')],
                CommandTreeNode::new_action(Message::New {
                    mode: NewMode::AfterTrunk,
                }),
            ),
            (
                "New",
                "After trunk (sync)",
                vec![KeyCode::Char('n'), KeyCode::Char('M')],
                CommandTreeNode::new_action(Message::NewAfterTrunkSync),
            ),
            (
                "New",
                "After target",
                vec![KeyCode::Char('n'), KeyCode::Char('/')],
                CommandTreeNode::new_action(Message::NewAtTarget),
            ),
            (
                "New",
                "After revsets",
                vec![KeyCode::Char('n'), KeyCode::Char('r')],
                CommandTreeNode::new_action(Message::NewRevsets),
            ),
            (
                "Commands",
                "Next",
                vec![KeyCode::Char('N')],
                CommandTreeNode::new_children(),
            ),
            (
                "Commands",
                "Open",
                vec![KeyCode::Char('o')],
                CommandTreeNode::new_action(Message::Open),
            ),
            (
                "Commands",
                "Resolve",
                vec![KeyCode::Char('O')],
                CommandTreeNode::new_action(Message::Resolve),
            ),
            (
                "Commands",
                "Parallelize",
                vec![KeyCode::Char('p')],
                CommandTreeNode::new_children(),
            ),
            (
                "Parallelize",
                "Selection with parent",
                vec![KeyCode::Char('p'), KeyCode::Char('p')],
                CommandTreeNode::new_action(Message::Parallelize {
                    source: ParallelizeSource::Selection,
                }),
            ),
            (
                "Parallelize",
                "From selection to destination",
                vec![KeyCode::Char('p'), KeyCode::Char('P')],
                CommandTreeNode::new_action_with_children(Message::SaveSelection),
            ),
            (
                "Parallelize range",
                "Select destination",
                vec![KeyCode::Char('p'), KeyCode::Char('P'), KeyCode::Enter],
                CommandTreeNode::new_action(Message::Parallelize {
                    source: ParallelizeSource::Range,
                }),
            ),
            (
                "Parallelize",
                "Revset",
                vec![KeyCode::Char('p'), KeyCode::Char('r')],
                CommandTreeNode::new_action(Message::Parallelize {
                    source: ParallelizeSource::Revset,
                }),
            ),
            (
                "Next",
                "Next",
                vec![KeyCode::Char('N'), KeyCode::Char('n')],
                CommandTreeNode::new_action(Message::NextPrev {
                    direction: NextPrevDirection::Next,
                    mode: NextPrevMode::Default,
                    offset: false,
                }),
            ),
            (
                "Next",
                "Nth next",
                vec![KeyCode::Char('N'), KeyCode::Char('N')],
                CommandTreeNode::new_action(Message::NextPrev {
                    direction: NextPrevDirection::Next,
                    mode: NextPrevMode::Default,
                    offset: true,
                }),
            ),
            (
                "Next",
                "Next (edit)",
                vec![KeyCode::Char('N'), KeyCode::Char('e')],
                CommandTreeNode::new_action(Message::NextPrev {
                    direction: NextPrevDirection::Next,
                    mode: NextPrevMode::Edit,
                    offset: false,
                }),
            ),
            (
                "Next",
                "Nth next (edit)",
                vec![KeyCode::Char('N'), KeyCode::Char('E')],
                CommandTreeNode::new_action(Message::NextPrev {
                    direction: NextPrevDirection::Next,
                    mode: NextPrevMode::Edit,
                    offset: true,
                }),
            ),
            (
                "Next",
                "Next (no-edit)",
                vec![KeyCode::Char('N'), KeyCode::Char('x')],
                CommandTreeNode::new_action(Message::NextPrev {
                    direction: NextPrevDirection::Next,
                    mode: NextPrevMode::NoEdit,
                    offset: false,
                }),
            ),
            (
                "Next",
                "Nth next (no-edit)",
                vec![KeyCode::Char('N'), KeyCode::Char('X')],
                CommandTreeNode::new_action(Message::NextPrev {
                    direction: NextPrevDirection::Next,
                    mode: NextPrevMode::NoEdit,
                    offset: true,
                }),
            ),
            (
                "Next",
                "Next conflict",
                vec![KeyCode::Char('N'), KeyCode::Char('c')],
                CommandTreeNode::new_action(Message::NextPrev {
                    direction: NextPrevDirection::Next,
                    mode: NextPrevMode::Conflict,
                    offset: false,
                }),
            ),
            (
                "Commands",
                "Previous",
                vec![KeyCode::Char('P')],
                CommandTreeNode::new_children(),
            ),
            (
                "Previous",
                "Previous",
                vec![KeyCode::Char('P'), KeyCode::Char('p')],
                CommandTreeNode::new_action(Message::NextPrev {
                    direction: NextPrevDirection::Prev,
                    mode: NextPrevMode::Default,
                    offset: false,
                }),
            ),
            (
                "Previous",
                "Nth previous",
                vec![KeyCode::Char('P'), KeyCode::Char('P')],
                CommandTreeNode::new_action(Message::NextPrev {
                    direction: NextPrevDirection::Prev,
                    mode: NextPrevMode::Default,
                    offset: true,
                }),
            ),
            (
                "Previous",
                "Previous (edit)",
                vec![KeyCode::Char('P'), KeyCode::Char('e')],
                CommandTreeNode::new_action(Message::NextPrev {
                    direction: NextPrevDirection::Prev,
                    mode: NextPrevMode::Edit,
                    offset: false,
                }),
            ),
            (
                "Previous",
                "Nth previous (edit)",
                vec![KeyCode::Char('P'), KeyCode::Char('E')],
                CommandTreeNode::new_action(Message::NextPrev {
                    direction: NextPrevDirection::Prev,
                    mode: NextPrevMode::Edit,
                    offset: true,
                }),
            ),
            (
                "Previous",
                "Previous (no-edit)",
                vec![KeyCode::Char('P'), KeyCode::Char('x')],
                CommandTreeNode::new_action(Message::NextPrev {
                    direction: NextPrevDirection::Prev,
                    mode: NextPrevMode::NoEdit,
                    offset: false,
                }),
            ),
            (
                "Previous",
                "Nth previous (no-edit)",
                vec![KeyCode::Char('P'), KeyCode::Char('X')],
                CommandTreeNode::new_action(Message::NextPrev {
                    direction: NextPrevDirection::Prev,
                    mode: NextPrevMode::NoEdit,
                    offset: true,
                }),
            ),
            (
                "Previous",
                "Previous conflict",
                vec![KeyCode::Char('P'), KeyCode::Char('c')],
                CommandTreeNode::new_action(Message::NextPrev {
                    direction: NextPrevDirection::Prev,
                    mode: NextPrevMode::Conflict,
                    offset: false,
                }),
            ),
            (
                "Commands",
                "Squash",
                vec![KeyCode::Char('s')],
                CommandTreeNode::new_children(),
            ),
            (
                "Squash",
                "Selection into parent",
                vec![KeyCode::Char('s'), KeyCode::Char('s')],
                CommandTreeNode::new_action(Message::Squash {
                    mode: SquashMode::Default,
                }),
            ),
            (
                "Squash",
                "Selection into destination",
                vec![KeyCode::Char('s'), KeyCode::Char('i')],
                CommandTreeNode::new_action_with_children(Message::SaveSelection),
            ),
            (
                "Squash into",
                "Select destination",
                vec![KeyCode::Char('s'), KeyCode::Char('i'), KeyCode::Enter],
                CommandTreeNode::new_action(Message::Squash {
                    mode: SquashMode::Into,
                }),
            ),
            (
                "Commands",
                "Status",
                vec![KeyCode::Char('t')],
                CommandTreeNode::new_action(Message::Status),
            ),
            (
                "Commands",
                "Sign",
                vec![KeyCode::Char('S')],
                CommandTreeNode::new_children(),
            ),
            (
                "Sign",
                "Selection",
                vec![KeyCode::Char('S'), KeyCode::Char('s')],
                CommandTreeNode::new_action(Message::Sign {
                    action: SignAction::Sign,
                    range: false,
                }),
            ),
            (
                "Sign",
                "From selection to destination",
                vec![KeyCode::Char('S'), KeyCode::Char('S')],
                CommandTreeNode::new_action_with_children(Message::SaveSelection),
            ),
            (
                "Sign range",
                "Select destination",
                vec![KeyCode::Char('S'), KeyCode::Char('S'), KeyCode::Enter],
                CommandTreeNode::new_action(Message::Sign {
                    action: SignAction::Sign,
                    range: true,
                }),
            ),
            (
                "Sign",
                "Unsign selection",
                vec![KeyCode::Char('S'), KeyCode::Char('u')],
                CommandTreeNode::new_action(Message::Sign {
                    action: SignAction::Unsign,
                    range: false,
                }),
            ),
            (
                "Sign",
                "Unsign from selection to destination",
                vec![KeyCode::Char('S'), KeyCode::Char('U')],
                CommandTreeNode::new_action_with_children(Message::SaveSelection),
            ),
            (
                "Unsign range",
                "Select destination",
                vec![KeyCode::Char('S'), KeyCode::Char('U'), KeyCode::Enter],
                CommandTreeNode::new_action(Message::Sign {
                    action: SignAction::Unsign,
                    range: true,
                }),
            ),
            (
                "Commands",
                "Simplify parents",
                vec![KeyCode::Char('y')],
                CommandTreeNode::new_children(),
            ),
            (
                "Simplify parents of",
                "Selection",
                vec![KeyCode::Char('y'), KeyCode::Char('y')],
                CommandTreeNode::new_action(Message::SimplifyParents {
                    mode: SimplifyParentsMode::Revisions,
                }),
            ),
            (
                "Simplify parents of",
                "Selection with descendants",
                vec![KeyCode::Char('y'), KeyCode::Char('Y')],
                CommandTreeNode::new_action(Message::SimplifyParents {
                    mode: SimplifyParentsMode::Source,
                }),
            ),
            (
                "Commands",
                "Rebase",
                vec![KeyCode::Char('r')],
                CommandTreeNode::new_children(),
            ),
            (
                "Rebase",
                "Selected branch",
                vec![KeyCode::Char('r'), KeyCode::Char('b')],
                CommandTreeNode::new_action_with_children(Message::SaveSelection),
            ),
            (
                "Rebase",
                "Selected branch onto trunk",
                vec![KeyCode::Char('r'), KeyCode::Char('m')],
                CommandTreeNode::new_action(Message::RebaseSelectedBranchOntoTrunk),
            ),
            (
                "Rebase",
                "Selected branch onto trunk (sync)",
                vec![KeyCode::Char('r'), KeyCode::Char('M')],
                CommandTreeNode::new_action(Message::RebaseSelectedBranchOntoTrunkSync),
            ),
            (
                "Rebase",
                "Selected source",
                vec![KeyCode::Char('r'), KeyCode::Char('s')],
                CommandTreeNode::new_action_with_children(Message::SaveSelection),
            ),
            (
                "Rebase",
                "Selected revision",
                vec![KeyCode::Char('r'), KeyCode::Char('r')],
                CommandTreeNode::new_action_with_children(Message::SaveSelection),
            ),
            (
                "Rebase",
                "Custom",
                vec![KeyCode::Char('r'), KeyCode::Char('c')],
                CommandTreeNode::new_action(Message::RebaseCustom),
            ),
            (
                "Rebase branch",
                "Insert after",
                vec![KeyCode::Char('r'), KeyCode::Char('b'), KeyCode::Char('a')],
                CommandTreeNode::new_children(),
            ),
            (
                "Rebase branch",
                "Insert before",
                vec![KeyCode::Char('r'), KeyCode::Char('b'), KeyCode::Char('b')],
                CommandTreeNode::new_children(),
            ),
            (
                "Rebase branch",
                "Onto",
                vec![KeyCode::Char('r'), KeyCode::Char('b'), KeyCode::Char('o')],
                CommandTreeNode::new_children(),
            ),
            (
                "Rebase branch after",
                "Select destination",
                vec![
                    KeyCode::Char('r'),
                    KeyCode::Char('b'),
                    KeyCode::Char('a'),
                    KeyCode::Enter,
                ],
                CommandTreeNode::new_action(Message::Rebase {
                    source_type: RebaseSourceType::Branch,
                    destination_type: RebaseDestinationType::InsertAfter,
                    destination: RebaseDestination::Selection,
                }),
            ),
            (
                "Rebase branch after",
                "Trunk",
                vec![
                    KeyCode::Char('r'),
                    KeyCode::Char('b'),
                    KeyCode::Char('a'),
                    KeyCode::Char('m'),
                ],
                CommandTreeNode::new_action(Message::Rebase {
                    source_type: RebaseSourceType::Branch,
                    destination_type: RebaseDestinationType::InsertAfter,
                    destination: RebaseDestination::Trunk,
                }),
            ),
            (
                "Rebase branch after",
                "@",
                vec![
                    KeyCode::Char('r'),
                    KeyCode::Char('b'),
                    KeyCode::Char('a'),
                    KeyCode::Char('c'),
                ],
                CommandTreeNode::new_action(Message::Rebase {
                    source_type: RebaseSourceType::Branch,
                    destination_type: RebaseDestinationType::InsertAfter,
                    destination: RebaseDestination::Current,
                }),
            ),
            (
                "Rebase branch after",
                "Target",
                vec![
                    KeyCode::Char('r'),
                    KeyCode::Char('b'),
                    KeyCode::Char('a'),
                    KeyCode::Char('/'),
                ],
                CommandTreeNode::new_action(Message::RebaseTargetFuzzy {
                    source_type: RebaseSourceType::Branch,
                    destination_type: RebaseDestinationType::InsertAfter,
                }),
            ),
            (
                "Rebase branch before",
                "Select destination",
                vec![
                    KeyCode::Char('r'),
                    KeyCode::Char('b'),
                    KeyCode::Char('b'),
                    KeyCode::Enter,
                ],
                CommandTreeNode::new_action(Message::Rebase {
                    source_type: RebaseSourceType::Branch,
                    destination_type: RebaseDestinationType::InsertBefore,
                    destination: RebaseDestination::Selection,
                }),
            ),
            (
                "Rebase branch before",
                "Trunk",
                vec![
                    KeyCode::Char('r'),
                    KeyCode::Char('b'),
                    KeyCode::Char('b'),
                    KeyCode::Char('m'),
                ],
                CommandTreeNode::new_action(Message::Rebase {
                    source_type: RebaseSourceType::Branch,
                    destination_type: RebaseDestinationType::InsertBefore,
                    destination: RebaseDestination::Trunk,
                }),
            ),
            (
                "Rebase branch before",
                "@",
                vec![
                    KeyCode::Char('r'),
                    KeyCode::Char('b'),
                    KeyCode::Char('b'),
                    KeyCode::Char('c'),
                ],
                CommandTreeNode::new_action(Message::Rebase {
                    source_type: RebaseSourceType::Branch,
                    destination_type: RebaseDestinationType::InsertBefore,
                    destination: RebaseDestination::Current,
                }),
            ),
            (
                "Rebase branch before",
                "Target",
                vec![
                    KeyCode::Char('r'),
                    KeyCode::Char('b'),
                    KeyCode::Char('b'),
                    KeyCode::Char('/'),
                ],
                CommandTreeNode::new_action(Message::RebaseTargetFuzzy {
                    source_type: RebaseSourceType::Branch,
                    destination_type: RebaseDestinationType::InsertBefore,
                }),
            ),
            (
                "Rebase branch onto",
                "Select destination",
                vec![
                    KeyCode::Char('r'),
                    KeyCode::Char('b'),
                    KeyCode::Char('o'),
                    KeyCode::Enter,
                ],
                CommandTreeNode::new_action(Message::Rebase {
                    source_type: RebaseSourceType::Branch,
                    destination_type: RebaseDestinationType::Onto,
                    destination: RebaseDestination::Selection,
                }),
            ),
            (
                "Rebase branch onto",
                "Trunk",
                vec![
                    KeyCode::Char('r'),
                    KeyCode::Char('b'),
                    KeyCode::Char('o'),
                    KeyCode::Char('m'),
                ],
                CommandTreeNode::new_action(Message::Rebase {
                    source_type: RebaseSourceType::Branch,
                    destination_type: RebaseDestinationType::Onto,
                    destination: RebaseDestination::Trunk,
                }),
            ),
            (
                "Rebase branch onto",
                "@",
                vec![
                    KeyCode::Char('r'),
                    KeyCode::Char('b'),
                    KeyCode::Char('o'),
                    KeyCode::Char('c'),
                ],
                CommandTreeNode::new_action(Message::Rebase {
                    source_type: RebaseSourceType::Branch,
                    destination_type: RebaseDestinationType::Onto,
                    destination: RebaseDestination::Current,
                }),
            ),
            (
                "Rebase branch onto",
                "Target",
                vec![
                    KeyCode::Char('r'),
                    KeyCode::Char('b'),
                    KeyCode::Char('o'),
                    KeyCode::Char('/'),
                ],
                CommandTreeNode::new_action(Message::RebaseTargetFuzzy {
                    source_type: RebaseSourceType::Branch,
                    destination_type: RebaseDestinationType::Onto,
                }),
            ),
            (
                "Rebase source",
                "Insert after",
                vec![KeyCode::Char('r'), KeyCode::Char('s'), KeyCode::Char('a')],
                CommandTreeNode::new_children(),
            ),
            (
                "Rebase source",
                "Insert before",
                vec![KeyCode::Char('r'), KeyCode::Char('s'), KeyCode::Char('b')],
                CommandTreeNode::new_children(),
            ),
            (
                "Rebase source",
                "Onto",
                vec![KeyCode::Char('r'), KeyCode::Char('s'), KeyCode::Char('o')],
                CommandTreeNode::new_children(),
            ),
            (
                "Rebase source after",
                "Select destination",
                vec![
                    KeyCode::Char('r'),
                    KeyCode::Char('s'),
                    KeyCode::Char('a'),
                    KeyCode::Enter,
                ],
                CommandTreeNode::new_action(Message::Rebase {
                    source_type: RebaseSourceType::Source,
                    destination_type: RebaseDestinationType::InsertAfter,
                    destination: RebaseDestination::Selection,
                }),
            ),
            (
                "Rebase source after",
                "Trunk",
                vec![
                    KeyCode::Char('r'),
                    KeyCode::Char('s'),
                    KeyCode::Char('a'),
                    KeyCode::Char('m'),
                ],
                CommandTreeNode::new_action(Message::Rebase {
                    source_type: RebaseSourceType::Source,
                    destination_type: RebaseDestinationType::InsertAfter,
                    destination: RebaseDestination::Trunk,
                }),
            ),
            (
                "Rebase source after",
                "@",
                vec![
                    KeyCode::Char('r'),
                    KeyCode::Char('s'),
                    KeyCode::Char('a'),
                    KeyCode::Char('c'),
                ],
                CommandTreeNode::new_action(Message::Rebase {
                    source_type: RebaseSourceType::Source,
                    destination_type: RebaseDestinationType::InsertAfter,
                    destination: RebaseDestination::Current,
                }),
            ),
            (
                "Rebase source after",
                "Target",
                vec![
                    KeyCode::Char('r'),
                    KeyCode::Char('s'),
                    KeyCode::Char('a'),
                    KeyCode::Char('/'),
                ],
                CommandTreeNode::new_action(Message::RebaseTargetFuzzy {
                    source_type: RebaseSourceType::Source,
                    destination_type: RebaseDestinationType::InsertAfter,
                }),
            ),
            (
                "Rebase source before",
                "Select destination",
                vec![
                    KeyCode::Char('r'),
                    KeyCode::Char('s'),
                    KeyCode::Char('b'),
                    KeyCode::Enter,
                ],
                CommandTreeNode::new_action(Message::Rebase {
                    source_type: RebaseSourceType::Source,
                    destination_type: RebaseDestinationType::InsertBefore,
                    destination: RebaseDestination::Selection,
                }),
            ),
            (
                "Rebase source before",
                "Trunk",
                vec![
                    KeyCode::Char('r'),
                    KeyCode::Char('s'),
                    KeyCode::Char('b'),
                    KeyCode::Char('m'),
                ],
                CommandTreeNode::new_action(Message::Rebase {
                    source_type: RebaseSourceType::Source,
                    destination_type: RebaseDestinationType::InsertBefore,
                    destination: RebaseDestination::Trunk,
                }),
            ),
            (
                "Rebase source before",
                "@",
                vec![
                    KeyCode::Char('r'),
                    KeyCode::Char('s'),
                    KeyCode::Char('b'),
                    KeyCode::Char('c'),
                ],
                CommandTreeNode::new_action(Message::Rebase {
                    source_type: RebaseSourceType::Source,
                    destination_type: RebaseDestinationType::InsertBefore,
                    destination: RebaseDestination::Current,
                }),
            ),
            (
                "Rebase source before",
                "Target",
                vec![
                    KeyCode::Char('r'),
                    KeyCode::Char('s'),
                    KeyCode::Char('b'),
                    KeyCode::Char('/'),
                ],
                CommandTreeNode::new_action(Message::RebaseTargetFuzzy {
                    source_type: RebaseSourceType::Source,
                    destination_type: RebaseDestinationType::InsertBefore,
                }),
            ),
            (
                "Rebase source onto",
                "Select destination",
                vec![
                    KeyCode::Char('r'),
                    KeyCode::Char('s'),
                    KeyCode::Char('o'),
                    KeyCode::Enter,
                ],
                CommandTreeNode::new_action(Message::Rebase {
                    source_type: RebaseSourceType::Source,
                    destination_type: RebaseDestinationType::Onto,
                    destination: RebaseDestination::Selection,
                }),
            ),
            (
                "Rebase source onto",
                "Trunk",
                vec![
                    KeyCode::Char('r'),
                    KeyCode::Char('s'),
                    KeyCode::Char('o'),
                    KeyCode::Char('m'),
                ],
                CommandTreeNode::new_action(Message::Rebase {
                    source_type: RebaseSourceType::Source,
                    destination_type: RebaseDestinationType::Onto,
                    destination: RebaseDestination::Trunk,
                }),
            ),
            (
                "Rebase source onto",
                "@",
                vec![
                    KeyCode::Char('r'),
                    KeyCode::Char('s'),
                    KeyCode::Char('o'),
                    KeyCode::Char('c'),
                ],
                CommandTreeNode::new_action(Message::Rebase {
                    source_type: RebaseSourceType::Source,
                    destination_type: RebaseDestinationType::Onto,
                    destination: RebaseDestination::Current,
                }),
            ),
            (
                "Rebase source onto",
                "Target",
                vec![
                    KeyCode::Char('r'),
                    KeyCode::Char('s'),
                    KeyCode::Char('o'),
                    KeyCode::Char('/'),
                ],
                CommandTreeNode::new_action(Message::RebaseTargetFuzzy {
                    source_type: RebaseSourceType::Source,
                    destination_type: RebaseDestinationType::Onto,
                }),
            ),
            (
                "Rebase revision",
                "Insert after",
                vec![KeyCode::Char('r'), KeyCode::Char('r'), KeyCode::Char('a')],
                CommandTreeNode::new_children(),
            ),
            (
                "Rebase revision",
                "Insert before",
                vec![KeyCode::Char('r'), KeyCode::Char('r'), KeyCode::Char('b')],
                CommandTreeNode::new_children(),
            ),
            (
                "Rebase revision",
                "Onto",
                vec![KeyCode::Char('r'), KeyCode::Char('r'), KeyCode::Char('o')],
                CommandTreeNode::new_children(),
            ),
            (
                "Rebase revision after",
                "Select destination",
                vec![
                    KeyCode::Char('r'),
                    KeyCode::Char('r'),
                    KeyCode::Char('a'),
                    KeyCode::Enter,
                ],
                CommandTreeNode::new_action(Message::Rebase {
                    source_type: RebaseSourceType::Revisions,
                    destination_type: RebaseDestinationType::InsertAfter,
                    destination: RebaseDestination::Selection,
                }),
            ),
            (
                "Rebase revision after",
                "Trunk",
                vec![
                    KeyCode::Char('r'),
                    KeyCode::Char('r'),
                    KeyCode::Char('a'),
                    KeyCode::Char('m'),
                ],
                CommandTreeNode::new_action(Message::Rebase {
                    source_type: RebaseSourceType::Revisions,
                    destination_type: RebaseDestinationType::InsertAfter,
                    destination: RebaseDestination::Trunk,
                }),
            ),
            (
                "Rebase revision after",
                "@",
                vec![
                    KeyCode::Char('r'),
                    KeyCode::Char('r'),
                    KeyCode::Char('a'),
                    KeyCode::Char('c'),
                ],
                CommandTreeNode::new_action(Message::Rebase {
                    source_type: RebaseSourceType::Revisions,
                    destination_type: RebaseDestinationType::InsertAfter,
                    destination: RebaseDestination::Current,
                }),
            ),
            (
                "Rebase revision after",
                "Target",
                vec![
                    KeyCode::Char('r'),
                    KeyCode::Char('r'),
                    KeyCode::Char('a'),
                    KeyCode::Char('/'),
                ],
                CommandTreeNode::new_action(Message::RebaseTargetFuzzy {
                    source_type: RebaseSourceType::Revisions,
                    destination_type: RebaseDestinationType::InsertAfter,
                }),
            ),
            (
                "Rebase revision before",
                "Select destination",
                vec![
                    KeyCode::Char('r'),
                    KeyCode::Char('r'),
                    KeyCode::Char('b'),
                    KeyCode::Enter,
                ],
                CommandTreeNode::new_action(Message::Rebase {
                    source_type: RebaseSourceType::Revisions,
                    destination_type: RebaseDestinationType::InsertBefore,
                    destination: RebaseDestination::Selection,
                }),
            ),
            (
                "Rebase revision before",
                "Trunk",
                vec![
                    KeyCode::Char('r'),
                    KeyCode::Char('r'),
                    KeyCode::Char('b'),
                    KeyCode::Char('m'),
                ],
                CommandTreeNode::new_action(Message::Rebase {
                    source_type: RebaseSourceType::Revisions,
                    destination_type: RebaseDestinationType::InsertBefore,
                    destination: RebaseDestination::Trunk,
                }),
            ),
            (
                "Rebase revision before",
                "@",
                vec![
                    KeyCode::Char('r'),
                    KeyCode::Char('r'),
                    KeyCode::Char('b'),
                    KeyCode::Char('c'),
                ],
                CommandTreeNode::new_action(Message::Rebase {
                    source_type: RebaseSourceType::Revisions,
                    destination_type: RebaseDestinationType::InsertBefore,
                    destination: RebaseDestination::Current,
                }),
            ),
            (
                "Rebase revision before",
                "Target",
                vec![
                    KeyCode::Char('r'),
                    KeyCode::Char('r'),
                    KeyCode::Char('b'),
                    KeyCode::Char('/'),
                ],
                CommandTreeNode::new_action(Message::RebaseTargetFuzzy {
                    source_type: RebaseSourceType::Revisions,
                    destination_type: RebaseDestinationType::InsertBefore,
                }),
            ),
            (
                "Rebase revision onto",
                "Select destination",
                vec![
                    KeyCode::Char('r'),
                    KeyCode::Char('r'),
                    KeyCode::Char('o'),
                    KeyCode::Enter,
                ],
                CommandTreeNode::new_action(Message::Rebase {
                    source_type: RebaseSourceType::Revisions,
                    destination_type: RebaseDestinationType::Onto,
                    destination: RebaseDestination::Selection,
                }),
            ),
            (
                "Rebase revision onto",
                "Trunk",
                vec![
                    KeyCode::Char('r'),
                    KeyCode::Char('r'),
                    KeyCode::Char('o'),
                    KeyCode::Char('m'),
                ],
                CommandTreeNode::new_action(Message::Rebase {
                    source_type: RebaseSourceType::Revisions,
                    destination_type: RebaseDestinationType::Onto,
                    destination: RebaseDestination::Trunk,
                }),
            ),
            (
                "Rebase revision onto",
                "@",
                vec![
                    KeyCode::Char('r'),
                    KeyCode::Char('r'),
                    KeyCode::Char('o'),
                    KeyCode::Char('c'),
                ],
                CommandTreeNode::new_action(Message::Rebase {
                    source_type: RebaseSourceType::Revisions,
                    destination_type: RebaseDestinationType::Onto,
                    destination: RebaseDestination::Current,
                }),
            ),
            (
                "Rebase revision onto",
                "Target",
                vec![
                    KeyCode::Char('r'),
                    KeyCode::Char('r'),
                    KeyCode::Char('o'),
                    KeyCode::Char('/'),
                ],
                CommandTreeNode::new_action(Message::RebaseTargetFuzzy {
                    source_type: RebaseSourceType::Revisions,
                    destination_type: RebaseDestinationType::Onto,
                }),
            ),
            (
                "Commands",
                "Restore",
                vec![KeyCode::Char('R')],
                CommandTreeNode::new_children(),
            ),
            (
                "Restore",
                "Changes in selection",
                vec![KeyCode::Char('R'), KeyCode::Char('r')],
                CommandTreeNode::new_action(Message::Restore {
                    mode: RestoreMode::ChangesIn,
                }),
            ),
            (
                "Restore",
                "Changes in selection (restore descendants)",
                vec![KeyCode::Char('R'), KeyCode::Char('d')],
                CommandTreeNode::new_action(Message::Restore {
                    mode: RestoreMode::ChangesInRestoreDescendants,
                }),
            ),
            (
                "Restore",
                "From selection into @",
                vec![KeyCode::Char('R'), KeyCode::Char('f')],
                CommandTreeNode::new_action(Message::Restore {
                    mode: RestoreMode::From,
                }),
            ),
            (
                "Restore",
                "From @ into selection",
                vec![KeyCode::Char('R'), KeyCode::Char('i')],
                CommandTreeNode::new_action(Message::Restore {
                    mode: RestoreMode::Into,
                }),
            ),
            (
                "Restore",
                "From selection into destination",
                vec![KeyCode::Char('R'), KeyCode::Char('R')],
                CommandTreeNode::new_action_with_children(Message::SaveSelection),
            ),
            (
                "Restore into",
                "Select destination",
                vec![KeyCode::Char('R'), KeyCode::Char('R'), KeyCode::Enter],
                CommandTreeNode::new_action(Message::Restore {
                    mode: RestoreMode::FromInto,
                }),
            ),
            (
                "Commands",
                "View",
                vec![KeyCode::Char('v')],
                CommandTreeNode::new_children(),
            ),
            (
                "View",
                "Selection",
                vec![KeyCode::Char('v'), KeyCode::Char('v')],
                CommandTreeNode::new_action(Message::View {
                    mode: ViewMode::Default,
                }),
            ),
            (
                "View",
                "From selection to @",
                vec![KeyCode::Char('v'), KeyCode::Char('f')],
                CommandTreeNode::new_action(Message::View {
                    mode: ViewMode::FromSelection,
                }),
            ),
            (
                "View",
                "From trunk to selection",
                vec![KeyCode::Char('v'), KeyCode::Char('m')],
                CommandTreeNode::new_action(Message::View {
                    mode: ViewMode::FromTrunkToSelection,
                }),
            ),
            (
                "View",
                "From @ to selection",
                vec![KeyCode::Char('v'), KeyCode::Char('t')],
                CommandTreeNode::new_action(Message::View {
                    mode: ViewMode::ToSelection,
                }),
            ),
            (
                "View",
                "From selection to destination",
                vec![KeyCode::Char('v'), KeyCode::Char('V')],
                CommandTreeNode::new_action_with_children(Message::SaveSelection),
            ),
            (
                "View to destination",
                "Select destination",
                vec![KeyCode::Char('v'), KeyCode::Char('V'), KeyCode::Enter],
                CommandTreeNode::new_action(Message::View {
                    mode: ViewMode::FromSelectionToDestination,
                }),
            ),
            (
                "Commands",
                "Revert",
                vec![KeyCode::Char('V')],
                CommandTreeNode::new_children(),
            ),
            (
                "Revert",
                "Selection onto @",
                vec![KeyCode::Char('V'), KeyCode::Char('v')],
                CommandTreeNode::new_action(Message::Revert {
                    revision: RevertRevision::Selection,
                    destination_type: RevertDestinationType::Onto,
                    destination: RevertDestination::Current,
                }),
            ),
            (
                "Revert",
                "Selection onto destination",
                vec![KeyCode::Char('V'), KeyCode::Char('o')],
                CommandTreeNode::new_action_with_children(Message::SaveSelection),
            ),
            (
                "Revert onto",
                "Select destination",
                vec![KeyCode::Char('V'), KeyCode::Char('o'), KeyCode::Enter],
                CommandTreeNode::new_action(Message::Revert {
                    revision: RevertRevision::Saved,
                    destination_type: RevertDestinationType::Onto,
                    destination: RevertDestination::Selection,
                }),
            ),
            (
                "Revert",
                "Selection after destination",
                vec![KeyCode::Char('V'), KeyCode::Char('a')],
                CommandTreeNode::new_action_with_children(Message::SaveSelection),
            ),
            (
                "Revert after",
                "Select destination",
                vec![KeyCode::Char('V'), KeyCode::Char('a'), KeyCode::Enter],
                CommandTreeNode::new_action(Message::Revert {
                    revision: RevertRevision::Saved,
                    destination_type: RevertDestinationType::InsertAfter,
                    destination: RevertDestination::Selection,
                }),
            ),
            (
                "Revert",
                "Selection before destination",
                vec![KeyCode::Char('V'), KeyCode::Char('b')],
                CommandTreeNode::new_action_with_children(Message::SaveSelection),
            ),
            (
                "Revert before",
                "Select destination",
                vec![KeyCode::Char('V'), KeyCode::Char('b'), KeyCode::Enter],
                CommandTreeNode::new_action(Message::Revert {
                    revision: RevertRevision::Saved,
                    destination_type: RevertDestinationType::InsertBefore,
                    destination: RevertDestination::Selection,
                }),
            ),
            (
                "Commands",
                "Workspace",
                vec![KeyCode::Char('w')],
                CommandTreeNode::new_children(),
            ),
            (
                "Workspace",
                "Add",
                vec![KeyCode::Char('w'), KeyCode::Char('a')],
                CommandTreeNode::new_children(),
            ),
            (
                "Workspace add",
                "By path (name from path)",
                vec![KeyCode::Char('w'), KeyCode::Char('a'), KeyCode::Char('a')],
                CommandTreeNode::new_action(Message::WorkspaceAddPathOnly),
            ),
            (
                "Workspace add",
                "By name and path",
                vec![KeyCode::Char('w'), KeyCode::Char('a'), KeyCode::Char('n')],
                CommandTreeNode::new_action(Message::WorkspaceAddNamed),
            ),
            (
                "Workspace",
                "Forget",
                vec![KeyCode::Char('w'), KeyCode::Char('f')],
                CommandTreeNode::new_children(),
            ),
            (
                "Workspace forget",
                "Current",
                vec![KeyCode::Char('w'), KeyCode::Char('f'), KeyCode::Char('c')],
                CommandTreeNode::new_action(Message::WorkspaceForgetCurrent),
            ),
            (
                "Workspace forget",
                "Target",
                vec![KeyCode::Char('w'), KeyCode::Char('f'), KeyCode::Char('/')],
                CommandTreeNode::new_action(Message::WorkspaceForgetFuzzy),
            ),
            (
                "Workspace forget",
                "All at selected change",
                vec![KeyCode::Char('w'), KeyCode::Char('f'), KeyCode::Char('f')],
                CommandTreeNode::new_action(Message::WorkspaceForgetAtSelection),
            ),
            (
                "Workspace",
                "List",
                vec![KeyCode::Char('w'), KeyCode::Char('L')],
                CommandTreeNode::new_action(Message::WorkspaceList),
            ),
            (
                "Workspace",
                "Rename current",
                vec![KeyCode::Char('w'), KeyCode::Char('r')],
                CommandTreeNode::new_action(Message::WorkspaceRename),
            ),
            (
                "Workspace",
                "Update stale",
                vec![KeyCode::Char('w'), KeyCode::Char('u')],
                CommandTreeNode::new_action(Message::WorkspaceUpdateStale),
            ),
            (
                "Commands",
                "Undo last operation",
                vec![KeyCode::Char('u')],
                CommandTreeNode::new_action(Message::Undo),
            ),
            (
                "Commands",
                "Redo last operation",
                vec![KeyCode::Char('U')],
                CommandTreeNode::new_action(Message::Redo),
            ),
        ];

        let mut tree = Self(CommandTreeNode::new_children());
        tree.add_children(items);
        tree
    }
}

fn render_help_text(entries: HelpEntries) -> Text<'static> {
    const COL_WIDTH: usize = 26;
    const MAX_ENTRIES_PER_COL: usize = 16;

    // Get lines for each column, splitting if over MAX_ENTRIES_PER_COL
    let columns: Vec<Vec<Line>> = entries
        .into_iter()
        .flat_map(|(group_help_text, help_group)| {
            let chunks: Vec<Vec<(String, String)>> = help_group
                .chunks(MAX_ENTRIES_PER_COL)
                .map(|c| c.to_vec())
                .collect();

            chunks.into_iter().enumerate().map(move |(i, chunk)| {
                let mut col_lines = Vec::new();
                // First chunk gets the header, subsequent chunks get blank header
                let header = if i == 0 {
                    group_help_text.clone()
                } else {
                    String::new()
                };
                col_lines.push(Line::from(vec![Span::styled(
                    format!("{header:COL_WIDTH$}"),
                    Style::default().fg(Color::Blue),
                )]));
                col_lines.extend(chunk.into_iter().map(|(key, help)| {
                    let mut num_cols = key.len() + 1 + help.len();
                    if !key.is_ascii() {
                        num_cols -= 2;
                    }
                    let padding = " ".repeat(COL_WIDTH.saturating_sub(num_cols));
                    Line::from(vec![
                        Span::styled(key, Style::default().fg(Color::Green)),
                        Span::raw(" "),
                        Span::raw(help),
                        Span::raw(padding),
                    ])
                }));
                col_lines
            })
        })
        .collect();

    // Render the columns
    let num_rows = columns.iter().map(|c| c.len()).max().unwrap();
    let lines: Vec<Line> = (0..num_rows)
        .map(|i| {
            let mut spans: Vec<Span> = vec![Span::raw(" ")];

            for col in &columns {
                let empty_line = Line::from(Span::raw(" ".repeat(COL_WIDTH)));
                let col_line = col.get(i).unwrap_or(&empty_line).clone();
                spans.extend(col_line.spans)
            }

            Line::from(spans)
        })
        .collect();

    lines.into()
}

pub fn display_unbound_error_lines(
    info_list: &mut Option<Text<'static>>,
    key_code: &KeyCode,
    clear_existing: bool,
) {
    let error_line = Line::from(vec![
        Span::styled(" Unbound suffix: ", Style::default().fg(Color::Red)),
        Span::raw("'"),
        Span::styled(format!("{key_code}"), Style::default().fg(Color::Green)),
        Span::raw("'"),
    ]);
    if clear_existing || info_list.is_none() {
        *info_list = Some(error_line.into());
    } else if let Some(info_list) = info_list {
        let add_blank_line = info_list.lines.first().unwrap().spans[0] != error_line.spans[0];
        if let Some(last_line) = info_list.lines.last()
            && !last_line.spans.is_empty()
            && last_line.spans[0] == error_line.spans[0]
        {
            info_list.lines.pop();
            info_list.lines.pop();
        }

        if add_blank_line {
            info_list.lines.push(Line::from(vec![]));
        }
        info_list.lines.push(error_line);
    }
}
