use crate::config::{Config, Theme};
use crate::keyboard::{KeyboardState, KeyboardWidget, split_with_keyboard};
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
use std::{
    io,
    num::NonZeroUsize,
    time::{Duration, Instant},
};

const POLL_IDLE: Duration = Duration::from_secs(3600);

const WORD_PRESETS: [usize; 6] = [10, 25, 50, 100, 200, 500];

const BANNER: &str = " ▄   ▄
▀█▀ ▀█▀ █ █ █▀█ █▀█
 █▄  █▄ █▄█ █▄█ █▄█
        ▄▄█ █";

fn banner_with_version() -> Vec<String> {
    let mut lines: Vec<String> = BANNER.lines().map(str::to_string).collect();
    if let Some(last) = lines.last_mut() {
        last.push_str(&format!(" v{}", env!("CARGO_PKG_VERSION")));
    }
    lines
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cursor {
    Language,
    Words,
    SuddenDeath,
    NoBacktrack,
    NoBackspace,
    Ascii,
}

impl Cursor {
    const ORDER: [Cursor; 6] = [
        Cursor::Language,
        Cursor::Words,
        Cursor::SuddenDeath,
        Cursor::NoBacktrack,
        Cursor::NoBackspace,
        Cursor::Ascii,
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
            cursor: Cursor::Language,
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
    kb: &mut KeyboardState,
    kb_visible: &mut bool,
) -> io::Result<Outcome> {
    loop {
        terminal.draw(|f| {
            let (display, kb_rect) = split_with_keyboard(f.area(), *kb_visible);
            f.render_widget(config.theme.apply_to(&title), display);
            if let Some(r) = kb_rect {
                f.render_widget(config.theme.apply_to(KeyboardWidget::new(kb)), r);
            }
        })?;

        let timeout = kb
            .next_deadline()
            .map(|d| d.saturating_duration_since(Instant::now()))
            .unwrap_or(POLL_IDLE);
        if !event::poll(timeout)? {
            kb.tick();
            continue;
        }
        let event = event::read()?;

        if let Event::Key(ke) = &event
            && ke.kind == KeyEventKind::Press
        {
            kb.note_event(ke);
        }

        if let Event::Key(KeyEvent {
            code: KeyCode::Char('c'),
            kind: KeyEventKind::Press,
            modifiers: KeyModifiers::CONTROL,
            ..
        }) = event
        {
            return Ok(Outcome::Quit);
        }

        // Toggle keyboard visibility on Ctrl+K (any phase).
        if let Event::Key(KeyEvent {
            code: KeyCode::Char('k'),
            kind: KeyEventKind::Press,
            modifiers: KeyModifiers::CONTROL,
            ..
        }) = event
        {
            *kb_visible = !*kb_visible;
            continue;
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
                    Cursor::SuddenDeath
                    | Cursor::NoBacktrack
                    | Cursor::NoBackspace
                    | Cursor::Ascii => title.toggle_current(),
                },
                KeyCode::Right | KeyCode::Char('l') => match title.cursor {
                    Cursor::Language => title.cycle_language(1),
                    Cursor::Words => title.next_word_preset(),
                    Cursor::SuddenDeath
                    | Cursor::NoBacktrack
                    | Cursor::NoBackspace
                    | Cursor::Ascii => title.toggle_current(),
                },
                KeyCode::Char(' ') => title.toggle_current(),
                KeyCode::Enter => match title.cursor {
                    Cursor::Language => title.open_picker(),
                    _ => return Ok(Outcome::Start(title)),
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

        match self.mode {
            Mode::Menu => {
                // Reserve the bottom row of the title area for the hint so it
                // is always visible, centered, and sits just above the
                // keyboard widget regardless of menu layout.
                let hint_h: u16 = if area.height >= 2 { 1 } else { 0 };
                let menu_area = Rect {
                    x: area.x,
                    y: area.y,
                    width: area.width,
                    height: area.height.saturating_sub(hint_h),
                };
                self.render_menu(menu_area, buf, theme, true);
                if hint_h > 0 {
                    let hint_rect = Rect {
                        x: area.x,
                        y: area.y + area.height - 1,
                        width: area.width,
                        height: 1,
                    };
                    let hint =
                        Line::from(Span::styled(self.hint_text(), theme.results_restart_prompt))
                            .alignment(Alignment::Center);
                    Paragraph::new(vec![hint]).render(hint_rect, buf);
                }
            }
            Mode::LanguagePicker => {
                // Picker stays a centered narrow card so the language list reads naturally.
                let card_w = 60u16.min(area.width);
                let card_h = 15u16.min(area.height);
                let card = Rect {
                    x: area.x + area.width.saturating_sub(card_w) / 2,
                    y: area.y + area.height.saturating_sub(card_h) / 2,
                    width: card_w,
                    height: card_h,
                };
                self.render_picker(card, buf, theme);
            }
        }
    }
}

impl Title {
    fn render_menu(&self, area: Rect, buf: &mut Buffer, theme: &Theme, include_banner: bool) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(theme.border_type)
            .border_style(theme.prompt_border)
            .padding(Padding::new(3, 3, 1, 1));
        let inner = block.inner(area);
        block.render(area, buf);

        let sel = |c: Cursor| c == self.cursor;
        // Render lines into a fixed-width column centered inside the card so
        // the pointer/label positions stay stable regardless of card width.
        let content_w: u16 = 50u16.min(inner.width);
        let inner_w = content_w as usize;
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

        let settings_lines: Vec<Line<'static>> = vec![
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
        ];
        let settings_h = settings_lines.len() as u16;

        let banner_strs = if include_banner {
            banner_with_version()
        } else {
            Vec::new()
        };
        let banner_w = banner_strs
            .iter()
            .map(|l| l.chars().count())
            .max()
            .unwrap_or(0) as u16;
        let banner_h = banner_strs.len() as u16;

        // Layout choice: stacked banner+settings if it all fits vertically;
        // otherwise side-by-side when the inner is wide enough; otherwise
        // settings only so nothing overflows the card.
        const COL_GAP: u16 = 4;
        let stacked_h = if banner_h > 0 {
            banner_h + 1 + settings_h
        } else {
            settings_h
        };
        let side_w = banner_w + COL_GAP + content_w;
        let side_h = banner_h.max(settings_h);

        if banner_h > 0 && inner.height >= stacked_h {
            let mut lines: Vec<Line> =
                Vec::with_capacity(banner_strs.len() + 1 + settings_lines.len());
            for l in &banner_strs {
                lines.push(
                    Line::from(Span::styled(
                        format!("{:<w$}", l, w = banner_w as usize),
                        theme.title,
                    ))
                    .alignment(Alignment::Center),
                );
            }
            lines.push(Line::from(""));
            lines.extend(settings_lines);
            let content_h = lines.len() as u16;
            let top_pad = inner.height.saturating_sub(content_h) / 2;
            let rect = Rect {
                x: inner.x + inner.width.saturating_sub(content_w) / 2,
                y: inner.y + top_pad,
                width: content_w,
                height: inner.height.saturating_sub(top_pad),
            };
            Paragraph::new(lines).render(rect, buf);
        } else if banner_h > 0 && inner.width >= side_w && inner.height >= side_h {
            let group_x = inner.x + inner.width.saturating_sub(side_w) / 2;
            let banner_lines: Vec<Line> = banner_strs
                .iter()
                .map(|l| Line::from(Span::styled(l.clone(), theme.title)))
                .collect();
            let banner_rect = Rect {
                x: group_x,
                y: inner.y + inner.height.saturating_sub(banner_h) / 2,
                width: banner_w,
                height: banner_h,
            };
            let settings_top_pad = inner.height.saturating_sub(settings_h) / 2;
            let settings_rect = Rect {
                x: group_x + banner_w + COL_GAP,
                y: inner.y + settings_top_pad,
                width: content_w,
                height: inner.height.saturating_sub(settings_top_pad),
            };
            Paragraph::new(banner_lines).render(banner_rect, buf);
            Paragraph::new(settings_lines).render(settings_rect, buf);
        } else {
            let top_pad = inner.height.saturating_sub(settings_h) / 2;
            let rect = Rect {
                x: inner.x + inner.width.saturating_sub(content_w) / 2,
                y: inner.y + top_pad,
                width: content_w,
                height: inner.height.saturating_sub(top_pad),
            };
            Paragraph::new(settings_lines).render(rect, buf);
        }
    }

    fn hint_text(&self) -> &'static str {
        match self.cursor {
            Cursor::Language => "h l cycle   ⏎ browse all   j k navigate",
            Cursor::Words => "h l preset   H L ±1   ⏎ start   j k navigate",
            Cursor::SuddenDeath | Cursor::NoBacktrack | Cursor::NoBackspace | Cursor::Ascii => {
                "h l space toggle   ⏎ start   j k navigate"
            }
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
        assert_eq!(Cursor::Language.prev(), Cursor::Ascii);
        assert_eq!(Cursor::Ascii.next(), Cursor::Language);
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
