use crate::config::{Config, Theme};
use crate::keyboard::{KeyboardArt, KeyboardState, KeyboardWidget, split_with_keyboard};
use crate::ui::ThemedWidget;
use std::collections::HashMap;

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
            cursor: Cursor::Words,
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

pub struct TitleWidget<'a> {
    pub title: &'a Title,
    pub kb: &'a KeyboardState,
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
            // Hide the keyboard automatically when the menu would otherwise
            // be cropped. Settings rows + a one-line hint plus the card's
            // border and padding need at least TITLE_MIN_AREA_H rows; if the
            // keyboard would steal space below that floor, drop it for this
            // frame without changing the user's preference.
            let art = KeyboardArt::embedded();
            let menu_fits_with_kb =
                f.area().height.saturating_sub(art.height) >= TITLE_MIN_AREA_H;
            let effective_kb_visible = *kb_visible && menu_fits_with_kb;
            let (display, kb_rect) = split_with_keyboard(f.area(), effective_kb_visible);
            let title_widget = TitleWidget { title: &title, kb };
            f.render_widget(config.theme.apply_to(&title_widget), display);
            if let Some(r) = kb_rect {
                let overrides = title.kb_label_overrides();
                f.render_widget(
                    config
                        .theme
                        .apply_to(KeyboardWidget::new(kb).with_overrides(overrides)),
                    r,
                );
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

        // Toggle keyboard visibility on Ctrl+k (any phase).
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

impl ThemedWidget for &TitleWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer, theme: &Theme) {
        buf.set_style(area, theme.default);

        match self.title.mode {
            Mode::Menu => self.title.render_menu(area, buf, theme, true, self.kb),
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
                self.title.render_picker(card, buf, theme);
            }
        }
    }
}

#[derive(Clone, Copy)]
struct HintKey {
    label: &'static str,
    // Matching label in keyboard.txt, used to look up press state so the
    // hint key flashes in sync with the keyboard widget.
    kb_label: &'static str,
    right_style: bool,
}

struct HintGroup {
    keys: &'static [HintKey],
    desc: &'static str,
}

const KEY_W: u16 = 7;
const KEY_H: u16 = 4;
const KEY_GAP: u16 = 1;
const BANNER_HINT_GAP: u16 = 1;
const BANNER_ENTER_GAP: u16 = 2;
const COL_GAP: u16 = 4;
const PREFERRED_CONTENT_W: u16 = 50;
const MIN_CONTENT_W: u16 = 30;
// Floor for the title area when the keyboard widget is visible. Card needs
// 7 settings rows + 1 hint row of inner content, plus 4 rows of border and
// padding (Padding::new(3, 3, 1, 1)). If the area-minus-keyboard falls below
// this, we hide the keyboard so the menu isn't truncated.
const TITLE_MIN_AREA_H: u16 = 12;

// Width of the wide Enter key art, matching the keyboard widget's Enter
// (`┏────────────┐┓` etc., 15 cells wide, 4 rows tall, with 12 inner cells).
const ENTER_W: u16 = 15;
const ENTER_INNER_W: usize = 12;

// H, J, K, L are all right-hand keys, so all use the right-style wing.
const H_L_KEYS: &[HintKey] = &[
    HintKey {
        label: "←H",
        kb_label: "H",
        right_style: true,
    },
    HintKey {
        label: "L→",
        kb_label: "L",
        right_style: true,
    },
];
const J_K_KEYS: &[HintKey] = &[
    HintKey {
        label: "J↓",
        kb_label: "J",
        right_style: true,
    },
    HintKey {
        label: "↑K",
        kb_label: "K",
        right_style: true,
    },
];

fn hint_groups_for(cursor: Cursor) -> &'static [HintGroup] {
    match cursor {
        Cursor::Language => &[
            HintGroup {
                keys: H_L_KEYS,
                desc: "cycle",
            },
            HintGroup {
                keys: J_K_KEYS,
                desc: "nav",
            },
        ],
        Cursor::Words => &[
            HintGroup {
                keys: H_L_KEYS,
                desc: "preset",
            },
            HintGroup {
                keys: J_K_KEYS,
                desc: "nav",
            },
        ],
        Cursor::SuddenDeath | Cursor::NoBacktrack | Cursor::NoBackspace | Cursor::Ascii => &[
            HintGroup {
                keys: H_L_KEYS,
                desc: "toggle",
            },
            HintGroup {
                keys: J_K_KEYS,
                desc: "nav",
            },
        ],
    }
}

fn enter_action_label(cursor: Cursor) -> &'static str {
    match cursor {
        Cursor::Language => "BROWSE",
        _ => "START",
    }
}

fn render_enter_key(
    action: &str,
    pressed: bool,
    origin_x: u16,
    origin_y: u16,
    buf: &mut Buffer,
    theme: &Theme,
) {
    let outline = theme.prompt_untyped;
    let label_style = theme.title;
    let pressed_style = theme.prompt_current_correct;
    let inner = ENTER_INNER_W as u16;

    debug_assert!(
        action.chars().count() + 3 <= ENTER_INNER_W,
        "action label too long for enter key"
    );
    let action_len = action.chars().count() as u16;

    let dashes = "─".repeat(ENTER_INNER_W);
    let dashes_long = "─".repeat(ENTER_INNER_W + 1);

    if pressed {
        // Wing flattens (┐→─, \→─). Right column keeps a single │ on rows 1, 2
        // and the label slot shifts right by one cell.
        let row0 = format!("┏{}─┓", dashes);
        let row2 = format!("├{}─│", dashes);
        let row3 = format!("┗{}┛", dashes_long);
        buf.set_string(origin_x, origin_y, &row0, pressed_style);
        buf.set_string(origin_x, origin_y + 2, &row2, outline);
        buf.set_string(origin_x, origin_y + 3, &row3, pressed_style);

        // Row 1: │, two leading spaces, ↵, space, action, trailing pad, │
        buf.set_string(origin_x, origin_y + 1, "│", pressed_style);
        buf.set_string(origin_x + 1, origin_y + 1, "  ", outline);
        buf.set_string(origin_x + 3, origin_y + 1, "↵", label_style);
        buf.set_string(origin_x + 4, origin_y + 1, " ", outline);
        buf.set_string(origin_x + 5, origin_y + 1, action, label_style);
        let pad_cells = inner.saturating_sub(4 + action_len);
        let pad = " ".repeat(pad_cells as usize);
        buf.set_string(origin_x + 5 + action_len, origin_y + 1, &pad, outline);
        buf.set_string(origin_x + 1 + inner, origin_y + 1, "│", pressed_style);
    } else {
        let row0 = format!("┏{}┐┓", dashes);
        let row2 = format!("├{}\\│", dashes);
        let row3 = format!("┗{}┛", dashes_long);
        buf.set_string(origin_x, origin_y, &row0, outline);
        buf.set_string(origin_x, origin_y + 2, &row2, outline);
        buf.set_string(origin_x, origin_y + 3, &row3, outline);

        // Row 1: │ ↵ <ACTION><pad>││
        buf.set_string(origin_x, origin_y + 1, "│ ", outline);
        buf.set_string(origin_x + 2, origin_y + 1, "↵", label_style);
        buf.set_string(origin_x + 3, origin_y + 1, " ", outline);
        buf.set_string(origin_x + 4, origin_y + 1, action, label_style);
        let pad_cells = inner.saturating_sub(3 + action_len);
        let pad = " ".repeat(pad_cells as usize);
        buf.set_string(origin_x + 4 + action_len, origin_y + 1, &pad, outline);
        buf.set_string(origin_x + 1 + inner, origin_y + 1, "││", outline);
    }
}

fn hint_group_keys_width(g: &HintGroup) -> u16 {
    let n = g.keys.len() as u16;
    n * KEY_W + n.saturating_sub(1) * KEY_GAP
}

fn hint_group_width(g: &HintGroup) -> u16 {
    let desc_w = g.desc.chars().count() as u16;
    hint_group_keys_width(g).max(desc_w)
}

fn render_hint_key(
    label: &str,
    right_style: bool,
    pressed: bool,
    origin_x: u16,
    origin_y: u16,
    buf: &mut Buffer,
    theme: &Theme,
) {
    let outline = theme.prompt_untyped;
    let label_style = theme.title;
    let pressed_style = theme.prompt_current_correct;
    let label_w = label.chars().count();
    debug_assert!(
        (1..=2).contains(&label_w),
        "hint key label must be 1 or 2 cells"
    );

    let static_label_style = if pressed { pressed_style } else { label_style };

    if right_style {
        if pressed {
            // Wing flattens: ┐→─, \→─. Right side keeps the lone │ on rows 1, 2.
            buf.set_string(origin_x, origin_y, "┏─────┓", pressed_style);
            buf.set_string(origin_x, origin_y + 2, "├─────│", outline);
            buf.set_string(origin_x, origin_y + 3, "┗─────┛", pressed_style);
            // Label row: shift label right by one, lose the inner │.
            // Cells: │ ' ' <leading_extra> <label> <trailing> │
            buf.set_string(origin_x, origin_y + 1, "│", pressed_style);
            buf.set_string(origin_x + 1, origin_y + 1, "  ", outline);
            let label_x = origin_x + 3;
            buf.set_string(label_x, origin_y + 1, label, static_label_style);
            // Right side of label row: spaces then a single │ (the wing │ is gone).
            let trail_w = 6 - 3 - label_w as u16;
            let suffix = format!("{}│", " ".repeat(trail_w as usize));
            buf.set_string(label_x + label_w as u16, origin_y + 1, &suffix, outline);
            // Force the col-6 │ in pressed style as well.
            buf.set_string(origin_x + 6, origin_y + 1, "│", pressed_style);
        } else {
            buf.set_string(origin_x, origin_y, "┏────┐┓", outline);
            buf.set_string(origin_x, origin_y + 2, "├────\\│", outline);
            buf.set_string(origin_x, origin_y + 3, "┗─────┛", outline);
            let leading = if label_w == 1 { 2 } else { 1 };
            let trailing = 4 - leading - label_w;
            let prefix = format!("│{}", " ".repeat(leading));
            buf.set_string(origin_x, origin_y + 1, &prefix, outline);
            let label_x = origin_x + 1 + leading as u16;
            buf.set_string(label_x, origin_y + 1, label, label_style);
            let suffix = format!("{}││", " ".repeat(trailing));
            buf.set_string(label_x + label_w as u16, origin_y + 1, &suffix, outline);
        }
    } else if pressed {
        // Wing flattens: ┌→─, /→─. Left side keeps the lone │ on rows 1, 2.
        buf.set_string(origin_x, origin_y, "┏─────┓", pressed_style);
        buf.set_string(origin_x, origin_y + 2, "│─────┤", outline);
        buf.set_string(origin_x, origin_y + 3, "┗─────┛", pressed_style);
        // Label row: shift label left by one, lose the inner │.
        buf.set_string(origin_x, origin_y + 1, "│", pressed_style);
        buf.set_string(origin_x + 1, origin_y + 1, " ", outline);
        let label_x = origin_x + 2;
        buf.set_string(label_x, origin_y + 1, label, static_label_style);
        let trail_w = 6 - 2 - label_w as u16;
        let suffix = format!("{}│", " ".repeat(trail_w as usize));
        buf.set_string(label_x + label_w as u16, origin_y + 1, &suffix, outline);
        buf.set_string(origin_x + 6, origin_y + 1, "│", pressed_style);
    } else {
        buf.set_string(origin_x, origin_y, "┏┌────┓", outline);
        buf.set_string(origin_x, origin_y + 2, "│/────┤", outline);
        buf.set_string(origin_x, origin_y + 3, "┗─────┛", outline);
        let trailing = 4 - 1 - label_w;
        buf.set_string(origin_x, origin_y + 1, "││ ", outline);
        let label_x = origin_x + 3;
        buf.set_string(label_x, origin_y + 1, label, label_style);
        let suffix = format!("{}│", " ".repeat(trailing));
        buf.set_string(label_x + label_w as u16, origin_y + 1, &suffix, outline);
    }
}

fn render_hint_group_below(
    group: &HintGroup,
    origin_x: u16,
    origin_y: u16,
    buf: &mut Buffer,
    theme: &Theme,
    kb: &KeyboardState,
) {
    let desc_style = theme.results_restart_prompt;

    let mut x = origin_x;
    for (ki, k) in group.keys.iter().enumerate() {
        if ki > 0 {
            x += KEY_GAP;
        }
        let pressed = kb.is_pressed(k.kb_label);
        render_hint_key(k.label, k.right_style, pressed, x, origin_y, buf, theme);
        x += KEY_W;
    }

    let keys_w = hint_group_keys_width(group);
    let desc_w = group.desc.chars().count() as u16;
    let desc_offset = if desc_w >= keys_w {
        0
    } else {
        (keys_w - desc_w) / 2
    };
    buf.set_string(
        origin_x + desc_offset,
        origin_y + KEY_H,
        group.desc,
        desc_style,
    );
}

impl Title {
    pub fn kb_label_overrides(&self) -> HashMap<String, String> {
        let mut m = HashMap::new();
        m.insert("H".into(), "←H".into());
        m.insert("L".into(), "L→".into());
        m.insert("J".into(), "J↓".into());
        m.insert("K".into(), "↑K".into());
        m.insert(
            "Enter".into(),
            format!("↵ {}", enter_action_label(self.cursor)),
        );
        m
    }

    fn render_menu(
        &self,
        area: Rect,
        buf: &mut Buffer,
        theme: &Theme,
        include_banner: bool,
        kb: &KeyboardState,
    ) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(theme.border_type)
            .border_style(theme.prompt_border)
            .padding(Padding::new(3, 3, 1, 1));
        let inner = block.inner(area);
        block.render(area, buf);

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

        let hints = hint_groups_for(self.cursor);
        debug_assert_eq!(hints.len(), 2, "expected H/L and J/K hint groups");
        let hints_h = if hints.is_empty() { 0 } else { KEY_H + 1 };
        let settings_h: u16 = 7;

        // Preferred layout: side-by-side with banner top-left, the wide Enter
        // key sitting to the right of the banner, key-art hints below them
        // (HL under the banner, JK under the Enter key) with descriptions
        // centered beneath each pair, and settings on the right. Pick
        // `content_w` to fit settings inside whatever room is left after the
        // left column. Falls back to stacked banner+settings with a bottom
        // text hint when there isn't room for the side-by-side form.
        let top_row_w = banner_w + BANNER_ENTER_GAP + ENTER_W;
        let jk_offset = banner_w + BANNER_ENTER_GAP;
        let hints_w = jk_offset + hints.get(1).map_or(0, hint_group_width);
        let left_w_target = top_row_w.max(hints_w);
        let left_h = banner_h.max(KEY_H) + BANNER_HINT_GAP + hints_h;
        let side_h = left_h.max(settings_h);
        let available_for_settings = inner.width.saturating_sub(left_w_target + COL_GAP);
        let side_by_side_fits =
            banner_h > 0 && available_for_settings >= MIN_CONTENT_W && inner.height >= side_h;

        let content_w: u16 = if side_by_side_fits {
            available_for_settings.min(PREFERRED_CONTENT_W)
        } else {
            PREFERRED_CONTENT_W.min(inner.width)
        };

        let sel = |c: Cursor| c == self.cursor;
        // Render lines into a fixed-width column centered inside the card so
        // the pointer/label positions stay stable regardless of card width.
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
        debug_assert_eq!(settings_lines.len() as u16, settings_h);

        let left_w = left_w_target;
        let side_w = left_w + COL_GAP + content_w;

        if side_by_side_fits {
            let group_x = inner.x + inner.width.saturating_sub(side_w) / 2;
            let group_top = inner.y + inner.height.saturating_sub(side_h) / 2;

            let banner_lines: Vec<Line> = banner_strs
                .iter()
                .map(|l| Line::from(Span::styled(l.clone(), theme.title)))
                .collect();
            // Vertically align the banner with the bottom of the Enter key
            // (both are 4 rows tall here, so the banner sits flush at the
            // top of the left column).
            let top_row_h = banner_h.max(KEY_H);
            let banner_y = group_top + top_row_h.saturating_sub(banner_h);
            let banner_rect = Rect {
                x: group_x,
                y: banner_y,
                width: banner_w,
                height: banner_h,
            };
            Paragraph::new(banner_lines).render(banner_rect, buf);

            render_enter_key(
                enter_action_label(self.cursor),
                kb.is_pressed("Enter"),
                group_x + banner_w + BANNER_ENTER_GAP,
                group_top,
                buf,
                theme,
            );

            let hint_y = group_top + top_row_h + BANNER_HINT_GAP;
            render_hint_group_below(&hints[0], group_x, hint_y, buf, theme, kb);
            render_hint_group_below(&hints[1], group_x + jk_offset, hint_y, buf, theme, kb);

            let settings_rect = Rect {
                x: group_x + left_w + COL_GAP,
                y: group_top + side_h.saturating_sub(settings_h) / 2,
                width: content_w,
                height: settings_h,
            };
            Paragraph::new(settings_lines).render(settings_rect, buf);
            return;
        }

        // Stacked-with-key-art fallback: when the side-by-side layout
        // doesn't fit but width and height still allow for the Enter + H/L +
        // J/K key art row (centered) above the settings, prefer that over a
        // bare text hint.
        let key_section_h = KEY_H + 1; // 4 rows of art + 1 desc row
        let hl_w = hint_group_width(&hints[0]);
        let jk_w = hint_group_width(&hints[1]);
        let key_row_w = ENTER_W + COL_GAP + hl_w + COL_GAP + jk_w;
        let stacked_keys_h = if banner_h > 0 {
            banner_h + BANNER_HINT_GAP + key_section_h + BANNER_HINT_GAP + settings_h
        } else {
            key_section_h + BANNER_HINT_GAP + settings_h
        };
        let stacked_keys_fits = inner.width >= key_row_w
            && inner.width >= banner_w
            && inner.width >= content_w
            && inner.height >= stacked_keys_h;

        if stacked_keys_fits {
            let group_top = inner.y + inner.height.saturating_sub(stacked_keys_h) / 2;
            let mut y = group_top;
            if banner_h > 0 {
                let banner_lines: Vec<Line> = banner_strs
                    .iter()
                    .map(|l| {
                        Line::from(Span::styled(
                            format!("{:<w$}", l, w = banner_w as usize),
                            theme.title,
                        ))
                        .alignment(Alignment::Center)
                    })
                    .collect();
                let banner_rect = Rect {
                    x: inner.x,
                    y,
                    width: inner.width,
                    height: banner_h,
                };
                Paragraph::new(banner_lines).render(banner_rect, buf);
                y += banner_h + BANNER_HINT_GAP;
            }

            // Order left-to-right: J/K (nav), H/L (preset), Enter (start).
            let row_x = inner.x + inner.width.saturating_sub(key_row_w) / 2;
            render_hint_group_below(&hints[1], row_x, y, buf, theme, kb);
            render_hint_group_below(&hints[0], row_x + jk_w + COL_GAP, y, buf, theme, kb);
            render_enter_key(
                enter_action_label(self.cursor),
                kb.is_pressed("Enter"),
                row_x + jk_w + COL_GAP + hl_w + COL_GAP,
                y,
                buf,
                theme,
            );
            y += key_section_h + BANNER_HINT_GAP;

            let settings_rect = Rect {
                x: inner.x + inner.width.saturating_sub(content_w) / 2,
                y,
                width: content_w,
                height: settings_h,
            };
            Paragraph::new(settings_lines).render(settings_rect, buf);
            return;
        }

        // Final fallback: reserve the bottom row for a single-line text hint,
        // then stack banner over settings (or settings only if the banner
        // does not fit).
        let hint_h: u16 = if inner.height >= 2 { 1 } else { 0 };
        let body_h = inner.height.saturating_sub(hint_h);
        let body_rect = Rect {
            x: inner.x,
            y: inner.y,
            width: inner.width,
            height: body_h,
        };
        let stacked_h = if banner_h > 0 {
            banner_h + 1 + settings_h
        } else {
            settings_h
        };

        if banner_h > 0 && body_rect.height >= stacked_h {
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
            let top_pad = body_rect.height.saturating_sub(content_h) / 2;
            let rect = Rect {
                x: body_rect.x + body_rect.width.saturating_sub(content_w) / 2,
                y: body_rect.y + top_pad,
                width: content_w,
                height: body_rect.height.saturating_sub(top_pad),
            };
            Paragraph::new(lines).render(rect, buf);
        } else {
            let top_pad = body_rect.height.saturating_sub(settings_h) / 2;
            let rect = Rect {
                x: body_rect.x + body_rect.width.saturating_sub(content_w) / 2,
                y: body_rect.y + top_pad,
                width: content_w,
                height: body_rect.height.saturating_sub(top_pad),
            };
            Paragraph::new(settings_lines).render(rect, buf);
        }

        if hint_h > 0 {
            let hint_rect = Rect {
                x: inner.x,
                y: inner.y + inner.height - 1,
                width: inner.width,
                height: 1,
            };
            let hint = Line::from(Span::styled(self.hint_text(), theme.results_restart_prompt))
                .alignment(Alignment::Center);
            Paragraph::new(vec![hint]).render(hint_rect, buf);
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
    fn kb_label_overrides_match_hint_labels() {
        let mut t = sample_title();
        t.cursor = Cursor::Language;
        let m = t.kb_label_overrides();
        assert_eq!(m.get("H").map(String::as_str), Some("←H"));
        assert_eq!(m.get("L").map(String::as_str), Some("L→"));
        assert_eq!(m.get("J").map(String::as_str), Some("J↓"));
        assert_eq!(m.get("K").map(String::as_str), Some("↑K"));
        assert_eq!(m.get("Enter").map(String::as_str), Some("↵ BROWSE"));
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
