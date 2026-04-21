use crate::config::{Config, Theme};
use crate::ui::ThemedWidget;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    buffer::Buffer,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Padding, Paragraph, Widget},
};
use std::{io, num::NonZeroUsize};

const WORD_PRESETS: [usize; 6] = [10, 25, 50, 100, 200, 500];

const BANNER: &str = " ▄   ▄
▀█▀ ▀█▀ █ █ █▀█ █▀█
 █▄  █▄ █▄█ █▄█ █▄█
        ▄▄█ █";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cursor {
    Language,
    Words,
    SuddenDeath,
    NoBacktrack,
    NoBackspace,
    Ascii,
    Start,
    Quit,
}

impl Cursor {
    const ORDER: [Cursor; 8] = [
        Cursor::Language,
        Cursor::Words,
        Cursor::SuddenDeath,
        Cursor::NoBacktrack,
        Cursor::NoBackspace,
        Cursor::Ascii,
        Cursor::Start,
        Cursor::Quit,
    ];

    fn next(self) -> Self {
        let i = Self::ORDER.iter().position(|c| *c == self).unwrap();
        Self::ORDER[(i + 1) % Self::ORDER.len()]
    }

    fn prev(self) -> Self {
        let i = Self::ORDER.iter().position(|c| *c == self).unwrap();
        Self::ORDER[(i + Self::ORDER.len() - 1) % Self::ORDER.len()]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Menu,
    LanguagePicker,
}

#[derive(Debug, Clone)]
pub struct Title {
    pub language: String,
    pub words: NonZeroUsize,
    pub sudden_death: bool,
    pub no_backtrack: bool,
    pub no_backspace: bool,
    pub ascii: bool,
    pub languages: Vec<String>,
    cursor: Cursor,
    mode: Mode,
    picker_filter: String,
    picker_cursor: usize,
}

pub enum Outcome {
    Start(Title),
    Quit,
}

impl Title {
    pub fn new(
        language: String,
        words: NonZeroUsize,
        sudden_death: bool,
        no_backtrack: bool,
        no_backspace: bool,
        ascii: bool,
        languages: Vec<String>,
    ) -> Self {
        Self {
            language,
            words,
            sudden_death,
            no_backtrack,
            no_backspace,
            ascii,
            languages,
            cursor: Cursor::Start,
            mode: Mode::Menu,
            picker_filter: String::new(),
            picker_cursor: 0,
        }
    }

    fn cycle_language(&mut self, delta: isize) {
        if self.languages.is_empty() {
            return;
        }
        let i = self
            .languages
            .iter()
            .position(|l| l == &self.language)
            .unwrap_or(0) as isize;
        let len = self.languages.len() as isize;
        let new = (i + delta).rem_euclid(len) as usize;
        self.language = self.languages[new].clone();
    }

    fn adjust_words(&mut self, delta: isize) {
        let n = (self.words.get() as isize + delta).max(1) as usize;
        if let Some(nz) = NonZeroUsize::new(n) {
            self.words = nz;
        }
    }

    fn next_word_preset(&mut self) {
        let cur = self.words.get();
        let next = WORD_PRESETS
            .iter()
            .find(|&&p| p > cur)
            .copied()
            .unwrap_or(WORD_PRESETS[0]);
        self.words = NonZeroUsize::new(next).unwrap();
    }

    fn prev_word_preset(&mut self) {
        let cur = self.words.get();
        let prev = WORD_PRESETS
            .iter()
            .rev()
            .find(|&&p| p < cur)
            .copied()
            .unwrap_or_else(|| *WORD_PRESETS.last().unwrap());
        self.words = NonZeroUsize::new(prev).unwrap();
    }

    fn toggle_current(&mut self) {
        match self.cursor {
            Cursor::SuddenDeath => self.sudden_death = !self.sudden_death,
            Cursor::NoBacktrack => self.no_backtrack = !self.no_backtrack,
            Cursor::NoBackspace => self.no_backspace = !self.no_backspace,
            Cursor::Ascii => self.ascii = !self.ascii,
            _ => {}
        }
    }

    fn filtered_languages(&self) -> impl Iterator<Item = &String> {
        let f = self.picker_filter.to_lowercase();
        self.languages
            .iter()
            .filter(move |l| f.is_empty() || l.to_lowercase().contains(&f))
    }

    fn filtered_count(&self) -> usize {
        self.filtered_languages().count()
    }

    fn open_picker(&mut self) {
        self.mode = Mode::LanguagePicker;
        self.picker_filter.clear();
        self.picker_cursor = self
            .languages
            .iter()
            .position(|l| l == &self.language)
            .unwrap_or(0);
    }

    fn commit_picker(&mut self) {
        let pick = self.filtered_languages().nth(self.picker_cursor).cloned();
        if let Some(name) = pick {
            self.language = name;
        }
        self.mode = Mode::Menu;
    }
}

pub fn run(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    config: &Config,
    mut title: Title,
) -> io::Result<Outcome> {
    loop {
        terminal.draw(|f| f.render_widget(config.theme.apply_to(&title), f.area()))?;
        let event = event::read()?;

        if let Event::Key(KeyEvent {
            code: KeyCode::Char('c'),
            kind: KeyEventKind::Press,
            modifiers: KeyModifiers::CONTROL,
            ..
        }) = event
        {
            return Ok(Outcome::Quit);
        }

        let Event::Key(KeyEvent {
            code,
            kind: KeyEventKind::Press,
            modifiers,
            ..
        }) = event
        else {
            continue;
        };

        match title.mode {
            Mode::Menu => match code {
                KeyCode::Esc | KeyCode::Char('q') => return Ok(Outcome::Quit),
                KeyCode::Up | KeyCode::Char('k') => title.cursor = title.cursor.prev(),
                KeyCode::Down | KeyCode::Char('j') => title.cursor = title.cursor.next(),
                // Shift+arrow or uppercase H/L: fine-tune words by ±1.
                KeyCode::Left if modifiers.contains(KeyModifiers::SHIFT) => {
                    if title.cursor == Cursor::Words {
                        title.adjust_words(-1);
                    }
                }
                KeyCode::Right if modifiers.contains(KeyModifiers::SHIFT) => {
                    if title.cursor == Cursor::Words {
                        title.adjust_words(1);
                    }
                }
                KeyCode::Char('H') => {
                    if title.cursor == Cursor::Words {
                        title.adjust_words(-1);
                    }
                }
                KeyCode::Char('L') => {
                    if title.cursor == Cursor::Words {
                        title.adjust_words(1);
                    }
                }
                // Plain arrow / h / l: language cycle or words preset cycle.
                KeyCode::Left | KeyCode::Char('h') => match title.cursor {
                    Cursor::Language => title.cycle_language(-1),
                    Cursor::Words => title.prev_word_preset(),
                    _ => {}
                },
                KeyCode::Right | KeyCode::Char('l') => match title.cursor {
                    Cursor::Language => title.cycle_language(1),
                    Cursor::Words => title.next_word_preset(),
                    _ => {}
                },
                KeyCode::Char(' ') => title.toggle_current(),
                KeyCode::Enter => match title.cursor {
                    Cursor::Language => title.open_picker(),
                    Cursor::Start => return Ok(Outcome::Start(title)),
                    Cursor::Quit => return Ok(Outcome::Quit),
                    Cursor::SuddenDeath
                    | Cursor::NoBacktrack
                    | Cursor::NoBackspace
                    | Cursor::Ascii => title.toggle_current(),
                    _ => {}
                },
                _ => {}
            },
            Mode::LanguagePicker => {
                let count = title.filtered_count();
                match code {
                    KeyCode::Esc => title.mode = Mode::Menu,
                    KeyCode::Enter => title.commit_picker(),
                    KeyCode::Up | KeyCode::Char('K') => {
                        if title.picker_cursor > 0 {
                            title.picker_cursor -= 1;
                        }
                    }
                    KeyCode::Down | KeyCode::Char('J') => {
                        if title.picker_cursor + 1 < count {
                            title.picker_cursor += 1;
                        }
                    }
                    KeyCode::PageUp => {
                        title.picker_cursor = title.picker_cursor.saturating_sub(10);
                    }
                    KeyCode::PageDown => {
                        title.picker_cursor =
                            (title.picker_cursor + 10).min(count.saturating_sub(1));
                    }
                    KeyCode::Backspace => {
                        title.picker_filter.pop();
                        title.picker_cursor = 0;
                    }
                    KeyCode::Char(c) if !modifiers.contains(KeyModifiers::CONTROL) => {
                        title.picker_filter.push(c);
                        title.picker_cursor = 0;
                    }
                    _ => {}
                }
            }
        }
    }
}

impl ThemedWidget for &Title {
    fn render(self, area: Rect, buf: &mut Buffer, theme: &Theme) {
        buf.set_style(area, theme.default);

        let card_w = 60u16.min(area.width);
        let card_h = 20u16.min(area.height);
        let card = Rect {
            x: area.x + area.width.saturating_sub(card_w) / 2,
            y: area.y + area.height.saturating_sub(card_h) / 2,
            width: card_w,
            height: card_h,
        };

        match self.mode {
            Mode::Menu => self.render_menu(card, buf, theme),
            Mode::LanguagePicker => self.render_picker(card, buf, theme),
        }
    }
}

impl Title {
    fn render_menu(&self, area: Rect, buf: &mut Buffer, theme: &Theme) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(theme.border_type)
            .border_style(theme.prompt_border)
            .padding(Padding::new(3, 3, 1, 1));
        let inner = block.inner(area);
        block.render(area, buf);

        let sel = |c: Cursor| c == self.cursor;
        let inner_w = inner.width as usize;
        let label_w: usize = 16;
        let pointer_w: usize = 2;

        let row_style = |c: Cursor| -> Style {
            if sel(c) {
                theme.prompt_current_untyped
            } else {
                theme.prompt_untyped
            }
        };
        let value_style = |c: Cursor| -> Style {
            if sel(c) {
                theme.prompt_current_correct
            } else {
                theme.prompt_untyped
            }
        };

        let setting_row = |c: Cursor, label: &str, value: Span<'static>| -> Line<'static> {
            let pointer = if sel(c) { "▸ " } else { "  " };
            let val_w = value.content.chars().count();
            let pad = inner_w.saturating_sub(pointer_w + label_w + val_w);
            Line::from(vec![
                Span::styled(pointer.to_string(), row_style(c)),
                Span::styled(format!("{:<w$}", label, w = label_w), row_style(c)),
                Span::raw(" ".repeat(pad)),
                value,
            ])
        };

        let bool_value = |c: Cursor, on: bool| -> Span<'static> {
            let text = if on { "on" } else { "off" };
            let base = if on {
                theme.prompt_correct
            } else {
                theme.prompt_untyped
            };
            let style = if sel(c) {
                base.add_modifier(Modifier::BOLD)
            } else {
                base
            };
            Span::styled(text, style)
        };

        let start_style = if sel(Cursor::Start) {
            theme.prompt_current_correct
        } else {
            theme.prompt_untyped
        };
        let quit_style = if sel(Cursor::Quit) {
            theme.prompt_current_incorrect
        } else {
            theme.prompt_untyped
        };

        let banner_w = BANNER
            .lines()
            .map(|l| l.chars().count())
            .max()
            .unwrap_or(0);
        let lines: Vec<Line> = BANNER
            .lines()
            .map(|l| {
                Line::from(Span::styled(
                    format!("{:<w$}", l, w = banner_w),
                    theme.title,
                ))
                .alignment(Alignment::Center)
            })
            .chain([
                Line::from(""),
                setting_row(
                    Cursor::Language,
                    "Language",
                    Span::styled(self.language.clone(), value_style(Cursor::Language)),
                ),
                setting_row(
                    Cursor::Words,
                    "Words",
                    Span::styled(format!("{}", self.words), value_style(Cursor::Words)),
                ),
                Line::from(""),
                setting_row(
                    Cursor::SuddenDeath,
                    "Sudden death",
                    bool_value(Cursor::SuddenDeath, self.sudden_death),
                ),
                setting_row(
                    Cursor::NoBacktrack,
                    "No backtrack",
                    bool_value(Cursor::NoBacktrack, self.no_backtrack),
                ),
                setting_row(
                    Cursor::NoBackspace,
                    "No backspace",
                    bool_value(Cursor::NoBackspace, self.no_backspace),
                ),
                setting_row(
                    Cursor::Ascii,
                    "ASCII only",
                    bool_value(Cursor::Ascii, self.ascii),
                ),
                Line::from(""),
                Line::from(vec![
                    Span::styled("[ Start ]", start_style),
                    Span::raw("    "),
                    Span::styled("[ Quit ]", quit_style),
                ])
                .alignment(Alignment::Center),
                Line::from(""),
                Line::from(Span::styled(self.hint_text(), theme.results_restart_prompt))
                    .alignment(Alignment::Center),
            ])
            .collect();

        Paragraph::new(lines).render(inner, buf);
    }

    fn hint_text(&self) -> &'static str {
        match self.cursor {
            Cursor::Language => {
                "h l cycle   ⏎ browse all   j k navigate"
            }
            Cursor::Words => {
                "h l preset   H L ±1   j k navigate"
            }
            Cursor::SuddenDeath | Cursor::NoBacktrack | Cursor::NoBackspace | Cursor::Ascii => {
                "space toggle   j k navigate"
            }
            Cursor::Start => "⏎ begin test   j k navigate",
            Cursor::Quit => "⏎ exit   j k navigate",
        }
    }

    fn render_picker(&self, area: Rect, buf: &mut Buffer, theme: &Theme) {
        let block = Block::default()
            .title(Span::styled(" select language ", theme.title))
            .borders(Borders::ALL)
            .border_type(theme.border_type)
            .border_style(theme.prompt_border)
            .padding(Padding::new(2, 2, 1, 1));

        let inner = block.inner(area);
        block.render(area, buf);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Min(1),
                Constraint::Length(1),
            ])
            .split(inner);

        let filter_line = Line::from(vec![
            Span::styled("filter: ", theme.prompt_untyped),
            Span::styled(self.picker_filter.clone(), theme.prompt_current_correct),
            Span::styled(" ", theme.prompt_cursor),
        ]);
        buf.set_line(chunks[0].x, chunks[0].y, &filter_line, chunks[0].width);

        let list_h = chunks[2].height as usize;
        let scroll = if self.picker_cursor >= list_h {
            self.picker_cursor + 1 - list_h
        } else {
            0
        };
        let filtered: Vec<&String> = self.filtered_languages().collect();
        for (i, name) in filtered.iter().skip(scroll).take(list_h).enumerate() {
            let idx = scroll + i;
            let (marker, style) = if idx == self.picker_cursor {
                ("▸ ", theme.prompt_current_correct)
            } else {
                ("  ", theme.prompt_untyped)
            };
            let line = Line::from(vec![
                Span::raw(marker.to_string()),
                Span::styled((*name).clone(), style),
            ]);
            buf.set_line(chunks[2].x, chunks[2].y + i as u16, &line, chunks[2].width);
        }

        let footer = Line::from(Span::styled(
            "type to filter  J K navigate  ⏎ select  esc back",
            theme.results_restart_prompt,
        ));
        buf.set_line(chunks[3].x, chunks[3].y, &footer, chunks[3].width);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_title() -> Title {
        Title::new(
            "english200".to_string(),
            NonZeroUsize::new(50).unwrap(),
            false,
            false,
            false,
            false,
            vec![
                "english100".into(),
                "english200".into(),
                "english1000".into(),
            ],
        )
    }

    #[test]
    fn cursor_next_prev_wraps() {
        assert_eq!(Cursor::Language.prev(), Cursor::Quit);
        assert_eq!(Cursor::Quit.next(), Cursor::Language);
        assert_eq!(Cursor::Language.next(), Cursor::Words);
    }

    #[test]
    fn cycle_language_wraps() {
        let mut t = sample_title();
        t.cycle_language(1);
        assert_eq!(t.language, "english1000");
        t.cycle_language(1);
        assert_eq!(t.language, "english100");
        t.cycle_language(-1);
        assert_eq!(t.language, "english1000");
    }

    #[test]
    fn adjust_words_floor_is_one() {
        let mut t = sample_title();
        t.adjust_words(-100);
        assert_eq!(t.words.get(), 1);
        t.adjust_words(10);
        assert_eq!(t.words.get(), 11);
    }

    #[test]
    fn word_preset_next_boundaries() {
        // [start, expected-after-next]: at-preset, between-presets, wrap, above-max
        for (start, expected) in [(50usize, 100), (73, 100), (500, 10), (1000, 10)] {
            let mut t = sample_title();
            t.words = NonZeroUsize::new(start).unwrap();
            t.next_word_preset();
            assert_eq!(t.words.get(), expected, "next from {}", start);
        }
    }

    #[test]
    fn word_preset_prev_boundaries() {
        // [start, expected-after-prev]: at-preset, between-presets, wrap-at-min
        for (start, expected) in [(100usize, 50), (73, 50), (10, 500)] {
            let mut t = sample_title();
            t.words = NonZeroUsize::new(start).unwrap();
            t.prev_word_preset();
            assert_eq!(t.words.get(), expected, "prev from {}", start);
        }
    }

    #[test]
    fn toggle_current_only_affects_bool_rows() {
        let mut t = sample_title();
        t.cursor = Cursor::Language;
        t.toggle_current();
        assert!(!t.sudden_death);

        t.cursor = Cursor::SuddenDeath;
        t.toggle_current();
        assert!(t.sudden_death);
        t.toggle_current();
        assert!(!t.sudden_death);

        t.cursor = Cursor::Ascii;
        t.toggle_current();
        assert!(t.ascii);
    }

    #[test]
    fn filter_narrows_list() {
        let mut t = sample_title();
        t.picker_filter = "1000".into();
        let matches: Vec<&String> = t.filtered_languages().collect();
        assert_eq!(matches, vec![&"english1000".to_string()]);
    }

    #[test]
    fn empty_filter_yields_all() {
        let t = sample_title();
        assert_eq!(t.filtered_count(), 3);
    }

    #[test]
    fn open_picker_positions_at_current() {
        let mut t = sample_title();
        t.open_picker();
        assert_eq!(t.mode, Mode::LanguagePicker);
        assert_eq!(t.picker_cursor, 1);
    }

    #[test]
    fn commit_picker_sets_language() {
        let mut t = sample_title();
        t.open_picker();
        t.picker_cursor = 2;
        t.commit_picker();
        assert_eq!(t.language, "english1000");
        assert_eq!(t.mode, Mode::Menu);
    }
}
