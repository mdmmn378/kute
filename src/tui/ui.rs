use std::collections::HashSet;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};

use super::form::{FieldKind, Form};
use super::{App, MENU, Screen};
use crate::model::GenKind;
use crate::search;

pub fn draw(frame: &mut Frame, app: &mut App) {
    let areas = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(0),
        Constraint::Length(3),
    ])
    .split(frame.area());

    draw_header(frame, areas[0], app);

    match app.screen {
        Screen::Menu => draw_menu(frame, areas[1], app),
        Screen::Search => draw_search(frame, areas[1], app),
        Screen::Generate => draw_generate(frame, areas[1], app),
        Screen::Scaffold => draw_editor(
            frame,
            areas[1],
            &app.scaffold.form,
            &app.scaffold.preview,
            app.scaffold.scroll,
            "Scaffold layout",
            "kustomize tree",
        ),
        Screen::Rbac => draw_editor(
            frame,
            areas[1],
            &app.rbac.form,
            &app.rbac.preview,
            app.rbac.scroll,
            "RBAC bundle",
            "manifest",
        ),
        Screen::Contexts => draw_contexts(frame, areas[1], app),
    }

    draw_footer(frame, areas[2], app);
}

fn draw_header(frame: &mut Frame, area: Rect, app: &App) {
    let line = Line::from(vec![
        Span::styled(
            " kute ",
            Style::default().fg(Color::Black).bg(Color::Cyan).bold(),
        ),
        Span::raw("  "),
        Span::styled(
            format!("context {}", app.context_label),
            Style::default().fg(Color::Green),
        ),
        Span::raw("  "),
        Span::styled(
            format!("ns {}", app.namespace_label),
            Style::default().fg(Color::Yellow),
        ),
    ]);

    let header = Paragraph::new(line).block(Block::default().borders(Borders::ALL));
    frame.render_widget(header, area);
}

fn draw_footer(frame: &mut Frame, area: Rect, app: &App) {
    let keys = match app.screen {
        Screen::Menu => "↑/↓ move · enter select · q quit",
        Screen::Search => "type to filter · ↑/↓ move · enter pick (prints on exit) · esc back",
        Screen::Generate if app.generate.choosing_kind => {
            "↑/↓ choose kind · enter apply · esc cancel"
        }
        Screen::Generate => {
            "↑/↓ field · enter edit · ←/→ cycle · k kind · r refresh · s save · esc back"
        }
        Screen::Scaffold => "↑/↓ field · enter edit · r refresh · s write · esc back",
        Screen::Rbac => "↑/↓ field · enter edit · ←/→ cycle · r refresh · s save · esc back",
        Screen::Contexts => "↑/↓ move · enter switch context · r refresh · esc back",
    };

    let status = match app.screen {
        Screen::Generate => &app.generate.message,
        Screen::Scaffold => &app.scaffold.message,
        Screen::Rbac => &app.rbac.message,
        _ => &app.status,
    };

    let line = if status.is_empty() {
        Line::from(Span::styled(keys, Style::default().fg(Color::DarkGray)))
    } else {
        Line::from(vec![
            Span::styled(status.clone(), Style::default().fg(Color::Green)),
            Span::raw("   "),
            Span::styled(keys, Style::default().fg(Color::DarkGray)),
        ])
    };

    frame.render_widget(
        Paragraph::new(line).block(Block::default().borders(Borders::ALL)),
        area,
    );
}

fn draw_menu(frame: &mut Frame, area: Rect, app: &mut App) {
    let items: Vec<ListItem> = MENU
        .iter()
        .map(|(title, description)| {
            ListItem::new(Line::from(vec![
                Span::styled(*title, Style::default().fg(Color::White).bold()),
                Span::raw("  "),
                Span::styled(*description, Style::default().fg(Color::DarkGray)),
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" what do you want to do? "),
        )
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");

    let mut state = ListState::default();
    state.select(Some(app.menu_index));
    frame.render_stateful_widget(list, area, &mut state);
}

fn draw_search(frame: &mut Frame, area: Rect, app: &mut App) {
    let rows = Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).split(area);

    let prompt = Paragraph::new(Line::from(vec![
        Span::styled(" search ", Style::default().fg(Color::Cyan).bold()),
        Span::raw(app.search.input.clone()),
        Span::styled("_", Style::default().fg(Color::Cyan)),
    ]))
    .block(Block::default().borders(Borders::ALL));
    frame.render_widget(prompt, rows[0]);

    let columns =
        Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)]).split(rows[1]);

    draw_search_results(frame, columns[0], app);
    draw_search_preview(frame, columns[1], app);
}

fn draw_search_results(frame: &mut Frame, area: Rect, app: &mut App) {
    let items: Vec<ListItem> = app
        .search
        .hits
        .iter()
        .map(|hit| {
            let candidate = &app.search.candidates[hit.index];
            let mut spans = command_spans(&candidate.cmd, &hit.positions);
            spans.push(Span::raw("  "));
            spans.push(Span::styled(
                candidate.origin.label(),
                Style::default().fg(match candidate.origin {
                    search::Origin::Example => Color::Blue,
                    search::Origin::History => Color::Magenta,
                }),
            ));
            if candidate.uses > 1 {
                spans.push(Span::styled(
                    format!(" ×{}", candidate.uses),
                    Style::default().fg(Color::DarkGray),
                ));
            }
            ListItem::new(Line::from(spans))
        })
        .collect();

    let title = if app.search.input.trim().is_empty() {
        format!(" all commands ({}) ", app.search.hits.len())
    } else {
        format!(" {} matches ", app.search.hits.len())
    };

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(title))
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");

    let mut state = ListState::default();
    if !app.search.hits.is_empty() {
        state.select(Some(app.search.selected));
    }
    frame.render_stateful_widget(list, area, &mut state);
}

fn draw_search_preview(frame: &mut Frame, area: Rect, app: &App) {
    let block = Block::default().borders(Borders::ALL).title(" preview ");
    let Some(candidate) = app.search.selected_candidate() else {
        frame.render_widget(
            Paragraph::new(Span::styled(
                "nothing selected",
                Style::default().fg(Color::DarkGray),
            ))
            .block(block),
            area,
        );
        return;
    };

    let mut lines = vec![
        Line::from(Span::styled(
            candidate.cmd.clone(),
            Style::default().fg(Color::Cyan).bold(),
        )),
        Line::raw(""),
    ];

    if !candidate.desc.is_empty() {
        lines.push(Line::from(candidate.desc.clone()));
        lines.push(Line::raw(""));
    }

    lines.push(Line::from(vec![
        Span::styled("category  ", Style::default().fg(Color::DarkGray)),
        Span::raw(candidate.category.clone()),
    ]));
    lines.push(Line::from(vec![
        Span::styled("source    ", Style::default().fg(Color::DarkGray)),
        Span::raw(candidate.origin.label()),
    ]));

    if !candidate.tags.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("tags      ", Style::default().fg(Color::DarkGray)),
            Span::raw(candidate.tags.join(", ")),
        ]));
    }

    if let Some(hit) = app.search.hits.get(app.search.selected) {
        lines.push(Line::from(vec![
            Span::styled("score     ", Style::default().fg(Color::DarkGray)),
            Span::raw(hit.score.to_string()),
        ]));
    }

    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .block(block)
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn draw_generate(frame: &mut Frame, area: Rect, app: &mut App) {
    if app.generate.choosing_kind {
        let columns = Layout::horizontal([Constraint::Percentage(45), Constraint::Percentage(55)])
            .split(area);

        let items: Vec<ListItem> = GenKind::ALL
            .iter()
            .map(|kind| {
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("{:<16}", kind.k8s_kind()),
                        Style::default().fg(Color::White).bold(),
                    ),
                    Span::styled(
                        kind.description().to_string(),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]))
            })
            .collect();

        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title(" kind "))
            .highlight_style(
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("> ");

        let mut state = ListState::default();
        state.select(Some(app.generate.kind_index));
        frame.render_stateful_widget(list, columns[0], &mut state);

        draw_preview(
            frame,
            columns[1],
            &app.generate.preview,
            app.generate.scroll,
            "manifest preview",
        );
        return;
    }

    let columns =
        Layout::horizontal([Constraint::Percentage(42), Constraint::Percentage(58)]).split(area);

    draw_form(
        frame,
        columns[0],
        &app.generate.form,
        &format!(" {} ", app.generate.kind().as_str()),
    );
    draw_preview(
        frame,
        columns[1],
        &app.generate.preview,
        app.generate.scroll,
        "manifest preview",
    );
}

fn draw_editor(
    frame: &mut Frame,
    area: Rect,
    form: &Form,
    preview: &str,
    scroll: u16,
    form_title: &str,
    preview_title: &str,
) {
    let columns =
        Layout::horizontal([Constraint::Percentage(42), Constraint::Percentage(58)]).split(area);

    draw_form(frame, columns[0], form, &format!(" {form_title} "));
    draw_preview(frame, columns[1], preview, scroll, preview_title);
}

fn draw_form(frame: &mut Frame, area: Rect, form: &Form, title: &str) {
    let lines: Vec<Line> = form
        .fields
        .iter()
        .enumerate()
        .map(|(index, field)| {
            let selected = index == form.selected;
            let marker = if selected { "> " } else { "  " };
            let label_style = if selected {
                Style::default().fg(Color::Cyan).bold()
            } else {
                Style::default().fg(Color::Gray)
            };

            let value = if field.kind == FieldKind::Choice {
                format!("‹ {} ›", field.value)
            } else {
                field.value.clone()
            };

            let value_style = if selected && form.editing {
                Style::default().fg(Color::Black).bg(Color::Cyan)
            } else if selected {
                Style::default().fg(Color::White).bold()
            } else {
                Style::default().fg(Color::White)
            };

            Line::from(vec![
                Span::styled(marker, Style::default().fg(Color::Cyan)),
                Span::styled(format!("{:<15}", field.label), label_style),
                Span::styled(value, value_style),
            ])
        })
        .collect();

    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .block(Block::default().borders(Borders::ALL).title(title))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn draw_preview(frame: &mut Frame, area: Rect, content: &str, scroll: u16, title: &str) {
    let paragraph = Paragraph::new(content.to_string())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" {title} ")),
        )
        .scroll((scroll, 0))
        .wrap(Wrap { trim: false });

    frame.render_widget(paragraph, area);
}

fn draw_contexts(frame: &mut Frame, area: Rect, app: &mut App) {
    if app.contexts.is_empty() {
        frame.render_widget(
            Paragraph::new(Text::from(vec![
                Line::raw("No kubectl contexts found."),
                Line::raw(""),
                Line::styled(
                    "Check that kubectl is installed and KUBECONFIG points at a valid file.",
                    Style::default().fg(Color::DarkGray),
                ),
            ]))
            .block(Block::default().borders(Borders::ALL).title(" contexts "))
            .wrap(Wrap { trim: false }),
            area,
        );
        return;
    }

    let items: Vec<ListItem> = app
        .contexts
        .iter()
        .map(|entry| {
            let marker = if entry.current { "* " } else { "  " };
            ListItem::new(Line::from(vec![
                Span::styled(
                    marker,
                    Style::default().fg(if entry.current {
                        Color::Green
                    } else {
                        Color::DarkGray
                    }),
                ),
                Span::styled(
                    entry.name.clone(),
                    if entry.current {
                        Style::default().fg(Color::Green).bold()
                    } else {
                        Style::default().fg(Color::White)
                    },
                ),
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" contexts · enter to switch "),
        )
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");

    let mut state = ListState::default();
    state.select(Some(app.context_index));
    frame.render_stateful_widget(list, area, &mut state);
}

/// Split a command into spans so fuzzy-matched characters can be emphasised.
fn command_spans(command: &str, positions: &[usize]) -> Vec<Span<'static>> {
    let matched: HashSet<usize> = positions.iter().copied().collect();

    let plain = Style::default().fg(Color::Gray);
    let hit = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);

    let mut spans = Vec::new();
    let mut buffer = String::new();
    let mut buffer_is_match = false;

    for (index, character) in command.chars().enumerate() {
        let is_match = matched.contains(&index);

        if is_match != buffer_is_match && !buffer.is_empty() {
            let style = if buffer_is_match { hit } else { plain };
            spans.push(Span::styled(std::mem::take(&mut buffer), style));
        }

        buffer_is_match = is_match;
        buffer.push(character);
    }

    if !buffer.is_empty() {
        let style = if buffer_is_match { hit } else { plain };
        spans.push(Span::styled(buffer, style));
    }

    spans
}
