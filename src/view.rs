use crate::model::{Model, State, TextInputSession};

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, Paragraph},
};

pub const SELECTION_COLOR: Color = Color::Rgb(40, 42, 54);
pub const SAVED_SELECTION_COLOR: Color = Color::Rgb(33, 35, 45);
const FUZZY_HIGHLIGHT_COLOR: Color = Color::Rgb(0xC9, 0x8E, 0x56);

pub fn view(model: &mut Model, frame: &mut Frame) {
    let header = render_header(model);
    let log_list = render_log_list(model);
    let layout = render_layout(model, frame.area());
    frame.render_widget(header, layout[0]);
    frame.render_stateful_widget(log_list, layout[1], &mut model.log_list_state);
    model.log_list_layout = layout[1];
    if model.state == State::EnteringText {
        render_text_input(model, frame, layout[2]);
    } else if let Some(info_list) = render_info_list(model) {
        frame.render_widget(info_list, layout[2]);
    }
}

fn render_layout(model: &Model, area: Rect) -> std::rc::Rc<[Rect]> {
    let bottom_height = if model.state == State::EnteringText {
        let fuzzy_lines = model
            .text_input
            .as_ref()
            .and_then(|s| s.fuzzy.as_ref())
            .map(|f| f.filtered.len() as u16)
            .unwrap_or(0);
        let divider = if fuzzy_lines > 0 { 1 } else { 0 };
        let base_height = 3 + fuzzy_lines + divider;
        base_height.min(area.height / 2)
    } else if let Some(info_list) = &model.info_list {
        info_list.lines.len() as u16 + 2
    } else {
        0
    };

    Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(0),
            Constraint::Length(bottom_height),
        ])
        .split(area)
}

fn render_header(model: &Model) -> Paragraph<'_> {
    let mut header_spans = vec![
        Span::styled("repository: ", Style::default().fg(Color::Blue)),
        Span::styled(&model.display_repository, Style::default().fg(Color::Green)),
        Span::raw("  "),
        Span::styled("revset: ", Style::default().fg(Color::Blue)),
        Span::styled(&model.revset, Style::default().fg(Color::Green)),
    ];
    if model.global_args.ignore_immutable {
        header_spans.push(Span::styled(
            "  --ignore-immutable",
            Style::default().fg(Color::LightRed),
        ));
    }
    Paragraph::new(Line::from(header_spans))
}

fn render_log_list(model: &Model) -> List<'static> {
    let mut log_items = model.log_list.clone();
    apply_saved_selection_highlights(model, &mut log_items);
    List::new(log_items)
        .highlight_style(Style::new().bold().bg(SELECTION_COLOR))
        .scroll_padding(model.log_list_scroll_padding)
}

fn apply_saved_selection_highlights(model: &Model, log_items: &mut [ratatui::text::Text<'static>]) {
    let (saved_commit_idx, saved_file_diff_idx) = model.get_saved_selection_flat_log_idxs();

    if let Some(idx) = saved_commit_idx
        && let Some(item) = log_items.get_mut(idx)
    {
        apply_saved_selection_highlight(item);
    }

    if let Some(idx) = saved_file_diff_idx
        && let Some(item) = log_items.get_mut(idx)
    {
        apply_saved_selection_highlight(item);
    }
}

fn apply_saved_selection_highlight(text: &mut ratatui::text::Text<'static>) {
    text.style = text.style.bg(SAVED_SELECTION_COLOR);
    for line in &mut text.lines {
        for span in &mut line.spans {
            span.style = span.style.bg(SAVED_SELECTION_COLOR);
        }
    }
}

fn render_info_list(model: &Model) -> Option<List<'static>> {
    let info_list = model.info_list.as_ref()?;
    Some(
        List::new(info_list.clone()).block(
            Block::default()
                .borders(Borders::TOP)
                .border_style(Style::default().fg(Color::Blue)),
        ),
    )
}

fn render_text_input(model: &mut Model, frame: &mut Frame, area: Rect) {
    let Some(text_input) = model.text_input.as_mut() else {
        return;
    };

    let input_block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(Color::Blue));
    let input_inner = input_block.inner(area);
    frame.render_widget(input_block, area);

    let (candidates_area, input_line_area) = if let Some(fuzzy) = &text_input.fuzzy
        && !fuzzy.filtered.is_empty()
        && input_inner.height > 3
    {
        let candidates_height = (fuzzy.filtered.len() as u16).min(input_inner.height - 3);
        let split = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(0),
                Constraint::Length(candidates_height),
                Constraint::Length(1),
                Constraint::Length(2),
            ])
            .split(input_inner);
        let divider = Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(Color::Blue));
        frame.render_widget(divider, split[2]);
        (Some(split[1]), split[3])
    } else {
        (None, input_inner)
    };

    if let Some(candidates_area) = candidates_area {
        render_fuzzy_candidates(text_input, frame, candidates_area);
    }

    render_prompt_and_textarea(text_input, frame, input_line_area);
}

fn render_fuzzy_candidates(text_input: &TextInputSession, frame: &mut Frame, area: Rect) {
    let Some(fuzzy) = &text_input.fuzzy else {
        return;
    };

    let visible_count = area.height as usize;
    let total = fuzzy.filtered.len();
    let default_start = total.saturating_sub(visible_count);
    let start = if fuzzy.selected < default_start {
        fuzzy.selected
    } else {
        default_start
    };
    let end = (start + visible_count).min(total);

    for (row_idx, filter_idx) in (start..end).enumerate() {
        let candidate = &fuzzy.filtered[filter_idx];
        let text = &fuzzy.candidates[candidate.candidate_index].display;
        let is_selected = filter_idx == fuzzy.selected;

        let line = build_highlighted_line(text, &candidate.match_positions, is_selected);

        let row_area = Rect {
            x: area.x,
            y: area.y + row_idx as u16,
            width: area.width,
            height: 1,
        };
        let paragraph = if is_selected {
            Paragraph::new(line).style(Style::default().bg(SELECTION_COLOR))
        } else {
            Paragraph::new(line)
        };
        frame.render_widget(paragraph, row_area);
    }
}

fn build_highlighted_line<'a>(
    text: &str,
    match_positions: &[usize],
    is_selected: bool,
) -> Line<'a> {
    let base_style = if is_selected {
        Style::default().bg(SELECTION_COLOR)
    } else {
        Style::default()
    };
    let match_style = base_style
        .fg(FUZZY_HIGHLIGHT_COLOR)
        .add_modifier(Modifier::BOLD);

    let mut spans = Vec::new();
    let mut last_end = 0;

    for &pos in match_positions {
        if pos > text.len() {
            continue;
        }
        let ch_len = text[pos..]
            .chars()
            .next()
            .map(|c| c.len_utf8())
            .unwrap_or(0);
        if ch_len == 0 {
            continue;
        }
        if pos > last_end {
            spans.push(Span::styled(text[last_end..pos].to_string(), base_style));
        }
        spans.push(Span::styled(
            text[pos..pos + ch_len].to_string(),
            match_style,
        ));
        last_end = pos + ch_len;
    }
    if last_end < text.len() {
        spans.push(Span::styled(text[last_end..].to_string(), base_style));
    }

    if is_selected {
        let indicator = Span::styled(
            "> ",
            Style::default()
                .fg(FUZZY_HIGHLIGHT_COLOR)
                .add_modifier(Modifier::BOLD),
        );
        spans.insert(0, indicator);
    } else {
        spans.insert(0, Span::raw("  "));
    }

    Line::from(spans)
}

fn render_prompt_and_textarea(text_input: &mut TextInputSession, frame: &mut Frame, area: Rect) {
    let prompt = format!("{}: ", text_input.prompt);
    let prompt_width = (prompt.len() as u16).min(area.width);
    if prompt_width > 0 {
        frame.render_widget(
            Paragraph::new(prompt).style(
                Style::default()
                    .fg(Color::Blue)
                    .add_modifier(Modifier::BOLD),
            ),
            Rect {
                x: area.x,
                y: area.y,
                width: prompt_width,
                height: area.height,
            },
        );
    }

    if area.width > prompt_width {
        text_input.textarea.set_block(Block::default());
        frame.render_widget(
            &text_input.textarea,
            Rect {
                x: area.x + prompt_width,
                y: area.y,
                width: area.width - prompt_width,
                height: area.height,
            },
        );
    }
}
