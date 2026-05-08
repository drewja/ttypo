//! Full-screen modal shown at startup when a resumable entry exists for the
//! input file. Follows the same pattern as `title::run`: blocks until the
//! user makes a decision.

use crate::config::Config;
use crate::progress::now_unix;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Widget},
};
use std::io;

pub struct ResumeInfo {
    pub source_label: String,
    pub word_index: usize,
    pub total_words: usize,
    pub updated_at: u64,
    pub hash_matches: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Start at the saved word index.
    Resume,
    /// Start fresh at word 0, keep the stored entry (will be overwritten on next save).
    Fresh,
    /// Start fresh and delete the stored entry.
    Discard,
    /// User hit Ctrl-C; abort entirely.
    Quit,
}

pub fn run(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    config: &Config,
    info: &ResumeInfo,
) -> io::Result<Outcome> {
    loop {
        terminal.draw(|f| {
            let area = f.area();
            Clear.render(area, f.buffer_mut());
            render(info, config, area, f.buffer_mut());
        })?;

        let Event::Key(KeyEvent {
            code,
            kind: KeyEventKind::Press,
            modifiers,
            ..
        }) = event::read()?
        else {
            continue;
        };

        if matches!(code, KeyCode::Char('c')) && modifiers.contains(KeyModifiers::CONTROL) {
            return Ok(Outcome::Quit);
        }
        match code {
            KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => return Ok(Outcome::Resume),
            KeyCode::Char('n') | KeyCode::Char('N') => return Ok(Outcome::Fresh),
            KeyCode::Char('d') | KeyCode::Char('D') if !info.hash_matches => {
                return Ok(Outcome::Discard);
            }
            KeyCode::Esc => return Ok(Outcome::Fresh),
            _ => {}
        }
    }
}

fn render(info: &ResumeInfo, config: &Config, area: Rect, buf: &mut ratatui::buffer::Buffer) {
    let percent = if info.total_words > 0 {
        (info.word_index as f64 / info.total_words as f64) * 100.0
    } else {
        0.0
    };

    let title_text = if info.hash_matches {
        " Resume? "
    } else {
        " File changed "
    };

    let title_style = if info.hash_matches {
        config.theme.title
    } else {
        config.theme.resume_prompt_warning
    };

    let position_line = format!(
        "Word {} of {} ({:.1}%)",
        format_thousands(info.word_index),
        format_thousands(info.total_words),
        percent,
    );

    let last_typed_line = format!("Last typed {}.", format_relative(info.updated_at));

    let emphasis = config.theme.resume_prompt_emphasis;
    let warning = config.theme.resume_prompt_warning;

    let mut lines: Vec<Line> = vec![
        Line::from(""),
        Line::from(Span::styled(info.source_label.clone(), emphasis))
            .alignment(Alignment::Center),
        Line::from(""),
    ];

    if !info.hash_matches {
        lines.push(
            Line::from(Span::styled(
                "File has changed since you last typed it.",
                warning,
            ))
            .alignment(Alignment::Center),
        );
        lines.push(Line::from(""));
    }

    lines.push(Line::from(position_line).alignment(Alignment::Center));
    lines.push(Line::from(last_typed_line).alignment(Alignment::Center));
    lines.push(Line::from(""));

    let prompt_line = if info.hash_matches {
        Line::from(vec![
            Span::styled("[Y]es", emphasis),
            Span::raw("    "),
            Span::styled("[N]o", emphasis),
        ])
    } else {
        Line::from(vec![
            Span::styled("[Y]es", emphasis),
            Span::raw("    "),
            Span::styled("[N]o", emphasis),
            Span::raw("    "),
            Span::styled("[D]iscard", emphasis),
        ])
    };
    lines.push(prompt_line.alignment(Alignment::Center));
    lines.push(Line::from(""));

    let inner_height = lines.len() as u16;
    let inner_width = 52u16;
    let box_width = inner_width.min(area.width.saturating_sub(2));
    let box_height = (inner_height + 2).min(area.height);

    let vert = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(box_height),
            Constraint::Min(0),
        ])
        .split(area);
    let horz = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(box_width),
            Constraint::Min(0),
        ])
        .split(vert[1]);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(config.theme.border_type)
        .border_style(config.theme.prompt_border)
        .title(Span::styled(title_text, title_style));

    Paragraph::new(lines)
        .block(block)
        .alignment(Alignment::Center)
        .render(horz[1], buf);
}

fn format_thousands(n: usize) -> String {
    let s = n.to_string();
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, &b) in bytes.iter().enumerate() {
        if i > 0 && (bytes.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(b as char);
    }
    out
}

fn format_relative(updated_at: u64) -> String {
    let now = now_unix();
    if updated_at == 0 || now <= updated_at {
        return "just now".into();
    }
    let secs = now - updated_at;
    match secs {
        0..=59 => "just now".into(),
        60..=3599 => {
            let m = secs / 60;
            if m == 1 {
                "1 minute ago".into()
            } else {
                format!("{} minutes ago", m)
            }
        }
        3600..=86_399 => {
            let h = secs / 3600;
            if h == 1 {
                "1 hour ago".into()
            } else {
                format!("{} hours ago", h)
            }
        }
        86_400..=604_799 => {
            let d = secs / 86_400;
            if d == 1 {
                "1 day ago".into()
            } else {
                format!("{} days ago", d)
            }
        }
        _ => {
            let w = secs / 604_800;
            if w == 1 {
                "1 week ago".into()
            } else {
                format!("{} weeks ago", w)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_thousands_small() {
        assert_eq!(format_thousands(0), "0");
        assert_eq!(format_thousands(42), "42");
        assert_eq!(format_thousands(999), "999");
    }

    #[test]
    fn format_thousands_large() {
        assert_eq!(format_thousands(1_000), "1,000");
        assert_eq!(format_thousands(12_345), "12,345");
        assert_eq!(format_thousands(267_000), "267,000");
        assert_eq!(format_thousands(1_234_567), "1,234,567");
    }

    #[test]
    fn format_relative_shapes() {
        let now = now_unix();
        assert_eq!(format_relative(now), "just now");
        assert_eq!(format_relative(now - 30), "just now");
        assert_eq!(format_relative(now - 60), "1 minute ago");
        assert_eq!(format_relative(now - 120), "2 minutes ago");
        assert_eq!(format_relative(now - 3600), "1 hour ago");
        assert_eq!(format_relative(now - 7200), "2 hours ago");
        assert_eq!(format_relative(now - 86400), "1 day ago");
        assert_eq!(format_relative(now - 86400 * 3), "3 days ago");
        assert_eq!(format_relative(now - 604800), "1 week ago");
        assert_eq!(format_relative(now - 604800 * 5), "5 weeks ago");
    }

    #[test]
    fn format_relative_zero_or_future_is_just_now() {
        assert_eq!(format_relative(0), "just now");
        assert_eq!(format_relative(now_unix() + 10_000), "just now");
    }
}
