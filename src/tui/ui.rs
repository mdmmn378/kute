use std::collections::HashSet;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};

use super::form::{Field, FieldKind, Form, RowRef, Section};
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

    frame.render_widget(
        Paragraph::new(line).block(Block::default().borders(Borders::ALL)),
        area,
    );
}

fn draw_footer(frame: &mut Frame, area: Rect, app: &App) {
    let keys = match app.screen {
        Screen::Menu => "↑/↓ move · enter select · q quit",
        Screen::Search => "type to filter · ↑/↓ move · enter pick (prints on exit) · esc back",
        Screen::Generate if app.generate.choosing_kind => {
            "↑/↓ choose kind · enter apply · esc cancel"
        }
        Screen::Generate => {
            "↑/↓ move · space toggle · enter edit · i common values · a add · d delete · k kind · s save"
        }
        Screen::Scaffold => {
            "↑/↓ move · space toggle section · enter edit · i common values · a add · d delete · s write"
        }
        Screen::Rbac => {
            "↑/↓ move · space toggle section · enter edit · i common values · a add · d delete · s save"
        }
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

// ── search ──────────────────────────────────────────────────────────────────

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

// ── generate / editors ──────────────────────────────────────────────────────

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
        Layout::horizontal([Constraint::Percentage(46), Constraint::Percentage(54)]).split(area);

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
        Layout::horizontal([Constraint::Percentage(46), Constraint::Percentage(54)]).split(area);

    draw_form(frame, columns[0], form, &format!(" {form_title} "));
    draw_preview(frame, columns[1], preview, scroll, preview_title);
}

/// Render the whole form: a header per section, then the fields of every
/// section that is currently switched on. An open value dropdown is drawn as a
/// popup on top.
fn draw_form(frame: &mut Frame, area: Rect, form: &Form, title: &str) {
    frame.render_widget(
        Paragraph::new(Text::from(form_lines(form)))
            .block(Block::default().borders(Borders::ALL).title(title))
            .wrap(Wrap { trim: false }),
        area,
    );

    if form.picker.is_some() {
        draw_picker(frame, area, form);
    }
}

/// The dropdown of common values: type to filter, `enter` to insert, `esc` to
/// go back to free-form editing.
fn draw_picker(frame: &mut Frame, area: Rect, form: &Form) {
    let Some(picker) = form.picker.as_ref() else {
        return;
    };

    let entries = form.picker_window();
    let label = form
        .find(picker.field_key)
        .map(|field| field.label)
        .unwrap_or("values");

    let height = (entries.len() as u16 + 4).clamp(5, area.height.saturating_sub(2).max(5));
    let popup = centered_rect(area, area.width.saturating_mul(88) / 100, height);

    frame.render_widget(Clear, popup);

    let lines: Vec<Line> = if entries.is_empty() {
        vec![Line::from(Span::styled(
            "  no matches — keep typing, or esc to type your own",
            Style::default().fg(Color::DarkGray),
        ))]
    } else {
        entries
            .iter()
            .map(|(value, selected)| {
                let style = if *selected {
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };
                let marker = if *selected { "> " } else { "  " };
                Line::from(vec![
                    Span::styled(marker, Style::default().fg(Color::Cyan)),
                    Span::styled(value.clone(), style),
                ])
            })
            .collect()
    };

    let title = format!(" {label} · enter insert · esc cancel ");
    let query = Line::from(vec![
        Span::styled("  filter: ", Style::default().fg(Color::DarkGray)),
        Span::styled(picker.query.clone(), Style::default().fg(Color::Cyan)),
        Span::styled("_", Style::default().fg(Color::Cyan)),
    ]);

    let mut block = Block::default().borders(Borders::ALL).title(title);
    block = block.border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let rows = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(inner);
    frame.render_widget(Paragraph::new(query), rows[0]);
    frame.render_widget(Paragraph::new(Text::from(lines)), rows[1]);
}

/// A rectangle of the given size, centred inside `area`.
fn centered_rect(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);

    Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    }
}

/// Build the form's visible lines. Split out from rendering so the layout --
/// which sections are skipped, which fields are reachable -- can be asserted
/// on directly in tests.
fn form_lines(form: &Form) -> Vec<Line<'static>> {
    let current = form.current_row();
    let mut lines: Vec<Line> = Vec::new();

    for (section_index, section) in form.sections.iter().enumerate() {
        let hidden = !form.section_applies(section);
        lines.push(section_header_line(
            section,
            Some(RowRef::Header(section_index)) == current,
            hidden,
        ));

        if hidden {
            continue;
        }

        if !section.enabled {
            lines.push(Line::from(Span::styled(
                "       skipped — press space to enable",
                Style::default().fg(Color::DarkGray),
            )));
            continue;
        }

        if !section.expanded {
            continue;
        }

        for (field_index, field) in section.fields.iter().enumerate() {
            let selected = Some(RowRef::Field(section_index, field_index)) == current;
            let editing = selected && form.editing;

            lines.push(field_line(field, selected, editing));

            if field.kind == FieldKind::List {
                lines.extend(list_lines(field, selected));
            }
        }
    }

    lines
}

fn section_header_line(section: &Section, selected: bool, hidden: bool) -> Line<'static> {
    let marker = if selected { "> " } else { "  " };
    let twisty = if section.expanded { "▾ " } else { "▸ " };

    let (title_style, checkbox_style) = if hidden {
        (
            Style::default().fg(Color::DarkGray),
            Style::default().fg(Color::DarkGray),
        )
    } else if section.enabled {
        (
            Style::default().fg(Color::Cyan).bold(),
            Style::default().fg(Color::Green).bold(),
        )
    } else {
        (
            Style::default().fg(Color::Gray),
            Style::default().fg(Color::DarkGray),
        )
    };

    let mut spans = vec![
        Span::styled(marker, Style::default().fg(Color::Cyan)),
        Span::styled(twisty, Style::default().fg(Color::DarkGray)),
    ];

    if section.optional {
        let checkbox = if section.enabled { "[x] " } else { "[ ] " };
        spans.push(Span::styled(checkbox, checkbox_style));
    }

    spans.push(Span::styled(section.title, title_style));

    let note = if hidden {
        "  (not used by this kind)".to_string()
    } else {
        format!("  {}", section.help)
    };
    spans.push(Span::styled(note, Style::default().fg(Color::DarkGray)));

    Line::from(spans)
}

fn field_line(field: &Field, selected: bool, editing: bool) -> Line<'static> {
    let marker = if selected { "  > " } else { "    " };
    let label_style = if selected {
        Style::default().fg(Color::Cyan).bold()
    } else {
        Style::default().fg(Color::Gray)
    };

    let mut spans = vec![
        Span::styled(marker, Style::default().fg(Color::Cyan)),
        Span::styled(format!("{:<16}", field.label), label_style),
    ];

    match field.kind {
        FieldKind::Toggle => {
            let (text, style) = if field.is_on() {
                ("[x] yes", Style::default().fg(Color::Green))
            } else {
                ("[ ] no", Style::default().fg(Color::DarkGray))
            };
            spans.push(Span::styled(text, style));
        }
        FieldKind::Choice => {
            let style = if selected {
                Style::default().fg(Color::White).bold()
            } else {
                Style::default().fg(Color::White)
            };
            spans.push(Span::styled(format!("‹ {} ›", field.value), style));
        }
        FieldKind::List => {
            let summary = if field.items.is_empty() {
                "(none — press a to add)".to_string()
            } else {
                format!(
                    "{} entr{}",
                    field.items.len(),
                    if field.items.len() == 1 { "y" } else { "ies" }
                )
            };
            spans.push(Span::styled(summary, Style::default().fg(Color::DarkGray)));
        }
        FieldKind::Text | FieldKind::Number => {
            let style = if editing {
                Style::default().fg(Color::Black).bg(Color::Cyan)
            } else if selected {
                Style::default().fg(Color::White).bold()
            } else {
                Style::default().fg(Color::White)
            };
            spans.push(Span::styled(field.value.clone(), style));
        }
    }

    if selected && !field.help.is_empty() && field.kind != FieldKind::List {
        spans.push(Span::styled(
            format!("  {}", field.help),
            Style::default().fg(Color::DarkGray),
        ));
    }

    // Advertise the palette on any field that has one, so the dropdown is
    // discoverable without reading the docs.
    if selected && field.has_suggestions() {
        spans.push(Span::styled(
            "  · i for common values",
            Style::default().fg(Color::Blue),
        ));
    }

    Line::from(spans)
}

/// List entries render as indented rows; the highlighted one is what `enter`,
/// `d` and typing act on.
fn list_lines(field: &Field, selected: bool) -> Vec<Line<'static>> {
    field
        .items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let active = selected && index == field.item_index;
            let marker = if active { "      ● " } else { "        " };

            let text = if item.is_empty() {
                Span::styled(
                    "(empty — type a value)".to_string(),
                    Style::default().fg(Color::DarkGray),
                )
            } else {
                let style = if active {
                    Style::default().fg(Color::Cyan).bold()
                } else {
                    Style::default().fg(Color::Gray)
                };
                Span::styled(item.clone(), style)
            };

            Line::from(vec![
                Span::styled(marker, Style::default().fg(Color::Cyan)),
                text,
            ])
        })
        .collect()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::form::{Field, Form, Section};

    /// Flatten a rendered line back to plain text for assertions.
    fn text(line: &Line<'_>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    fn rendered(form: &Form) -> Vec<String> {
        form_lines(form).iter().map(text).collect()
    }

    fn sample() -> Form {
        Form::new(vec![
            Section::required(
                "Basics",
                "always required",
                vec![Field::text("name", "name", "app", "")],
            ),
            Section::optional(
                "Ingress",
                "route traffic",
                vec![
                    Field::text("host", "host", "", ""),
                    Field::toggle("enabled", "enabled", true, ""),
                    Field::list("hosts", "hosts", ""),
                ],
            )
            .for_kinds(&[GenKind::Ingress]),
        ])
    }

    #[test]
    fn a_skipped_section_offers_to_be_enabled() {
        let lines = rendered(&sample());

        assert!(lines.iter().any(|line| line.contains("[ ] Ingress")));
        assert!(
            lines
                .iter()
                .any(|line| line.contains("skipped — press space to enable"))
        );
        // Its fields stay hidden while it is off.
        assert!(
            !lines
                .iter()
                .any(|line| line.contains("route traffic  host"))
        );
    }

    #[test]
    fn an_enabled_section_renders_its_fields() {
        let mut form = sample();
        form.rows(); // settle the cursor
        form.row = 2; // the Ingress header
        form.handle_key(ratatui::crossterm::event::KeyCode::Char(' '));

        let lines = rendered(&form);

        assert!(lines.iter().any(|line| line.contains("[x] Ingress")));
        assert!(lines.iter().any(|line| line.contains("host")));
        assert!(
            lines
                .iter()
                .any(|line| line.contains("‹") || line.contains("enabled"))
        );
        assert!(
            !lines
                .iter()
                .any(|line| line.contains("skipped — press space to enable"))
        );
    }

    #[test]
    fn a_section_irrelevant_to_the_kind_is_marked_not_used() {
        let form = sample().with_kind(GenKind::Deployment);
        let lines = rendered(&form);

        assert!(
            lines
                .iter()
                .any(|line| line.contains("Ingress") && line.contains("(not used by this kind)")),
            "expected an explanatory marker, got: {lines:#?}"
        );
    }

    #[test]
    fn toggle_fields_render_as_checkboxes() {
        let mut form = sample();
        form.row = 2;
        form.handle_key(ratatui::crossterm::event::KeyCode::Enter);

        let lines = rendered(&form);
        assert!(lines.iter().any(|line| line.contains("[x] yes")));
    }

    #[test]
    fn list_fields_render_their_entries() {
        let mut form = sample();
        form.row = 2;
        form.handle_key(ratatui::crossterm::event::KeyCode::Enter);

        let lines = rendered(&form);
        assert!(
            lines
                .iter()
                .any(|line| line.contains("(none — press a to add)")),
            "empty lists should advertise the add key"
        );

        form.row = 5; // the list field
        form.handle_key(ratatui::crossterm::event::KeyCode::Char('a'));
        for character in "a.example.com".chars() {
            form.handle_key(ratatui::crossterm::event::KeyCode::Char(character));
        }
        form.handle_key(ratatui::crossterm::event::KeyCode::Enter);

        let lines = rendered(&form);
        assert!(lines.iter().any(|line| line.contains("a.example.com")));
        assert!(lines.iter().any(|line| line.contains("1 entry")));
    }
}
