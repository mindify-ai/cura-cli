use std::{io, time::Duration};

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
};

#[derive(Debug, Clone, Copy)]
pub enum DashboardAction {
    Install,
    Environments,
    Use,
    Doctor,
    Quit,
}

pub fn dashboard(installed: usize, active: Option<&str>) -> Result<DashboardAction> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let result = dashboard_loop(&mut terminal, installed, active);
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    result
}

fn dashboard_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    installed: usize,
    active: Option<&str>,
) -> Result<DashboardAction> {
    let actions = [
        ("Install CUDA", "Browse releases and choose a profile"),
        ("Environments", "View installed CUDA toolkits"),
        ("Switch version", "Set project or global CUDA"),
        ("Run doctor", "Check GPU, driver, and shell health"),
        ("Quit", "Leave CURA"),
    ];
    let mut selected = 0usize;
    loop {
        terminal.draw(|frame| {
            let area = frame.area();
            frame.render_widget(
                Block::default().style(Style::default().bg(Color::Rgb(7, 12, 24))),
                area,
            );
            let centered = centered_rect(78, 31, area);
            frame.render_widget(Clear, centered);
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(5),
                    Constraint::Length(4),
                    Constraint::Min(12),
                    Constraint::Length(2),
                ])
                .split(centered);
            let title = Paragraph::new(vec![
                Line::from(vec![
                    Span::styled(
                        "CURA",
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        "  CUDA environment manager",
                        Style::default().fg(Color::Gray),
                    ),
                ]),
                Line::from(Span::styled(
                    "Install. Switch. Accelerate.",
                    Style::default().fg(Color::Rgb(139, 148, 158)),
                )),
            ])
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::BOTTOM)
                    .border_style(Style::default().fg(Color::DarkGray)),
            );
            frame.render_widget(title, chunks[0]);
            let status = Paragraph::new(Line::from(vec![
                Span::styled(
                    format!("  {installed} "),
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" environments     "),
                Span::styled(
                    "ACTIVE ",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(active.unwrap_or("none")),
            ]))
            .block(
                Block::default()
                    .title(" Overview ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Rgb(40, 55, 75))),
            );
            frame.render_widget(status, chunks[1]);
            let items: Vec<ListItem> = actions
                .iter()
                .enumerate()
                .map(|(i, (name, description))| {
                    let marker = if i == selected { "›" } else { " " };
                    ListItem::new(Line::from(vec![
                        Span::styled(
                            format!(" {marker} {name:<18}"),
                            if i == selected {
                                Style::default()
                                    .fg(Color::Cyan)
                                    .add_modifier(Modifier::BOLD)
                            } else {
                                Style::default().fg(Color::White)
                            },
                        ),
                        Span::styled(*description, Style::default().fg(Color::DarkGray)),
                    ]))
                })
                .collect();
            let mut state = ListState::default().with_selected(Some(selected));
            frame.render_stateful_widget(
                List::new(items)
                    .block(
                        Block::default()
                            .title(" Actions ")
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(Color::Rgb(40, 55, 75))),
                    )
                    .highlight_style(Style::default().bg(Color::Rgb(13, 30, 48))),
                chunks[2],
                &mut state,
            );
            frame.render_widget(
                Paragraph::new("↑/↓ navigate   enter select   q quit")
                    .alignment(Alignment::Center)
                    .style(Style::default().fg(Color::DarkGray)),
                chunks[3],
            );
        })?;
        if event::poll(Duration::from_millis(250))?
            && let Event::Key(key) = event::read()?
        {
            if key.kind != KeyEventKind::Press {
                continue;
            }
            match key.code {
                KeyCode::Up | KeyCode::Char('k') => selected = selected.saturating_sub(1),
                KeyCode::Down | KeyCode::Char('j') => {
                    selected = (selected + 1).min(actions.len() - 1)
                }
                KeyCode::Char('q') | KeyCode::Esc => return Ok(DashboardAction::Quit),
                KeyCode::Enter => {
                    return Ok(match selected {
                        0 => DashboardAction::Install,
                        1 => DashboardAction::Environments,
                        2 => DashboardAction::Use,
                        3 => DashboardAction::Doctor,
                        _ => DashboardAction::Quit,
                    });
                }
                _ => {}
            }
        }
    }
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let width = width.min(area.width.saturating_sub(2));
    let height = height.min(area.height.saturating_sub(1));
    Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    }
}
