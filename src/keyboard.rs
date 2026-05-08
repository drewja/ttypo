use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{buffer::Buffer, layout::Rect};

use crate::config::Theme;
use crate::ui::ThemedWidget;

const KEYBOARD_ART: &str = include_str!("../resources/keyboard.txt");
pub const FLASH_DURATION: Duration = Duration::from_millis(140);

const TOP_LEFT: char = '┏';
const BOTTOM_LEFT: char = '┗';
const BOTTOM_RIGHT: char = '┛';
const TOP_LEFT_INNER: char = '┌';
const TOP_RIGHT_INNER: char = '┐';
const VBAR: char = '│';
const HBAR: char = '─';

fn is_single_letter(label: &str) -> Option<char> {
    let mut chars = label.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) if c.is_alphabetic() => Some(c),
        _ => None,
    }
}

#[derive(Debug, Clone)]
struct KeyInst {
    r: usize,
    c: usize,
    right: usize,
    bottom: usize,
    has_left: bool,
    has_right: bool,
    label_col: usize,
}

pub struct KeyboardArt {
    grid: Vec<Vec<char>>,
    instances: HashMap<String, Vec<KeyInst>>,
    pub width: u16,
    pub height: u16,
}

impl KeyboardArt {
    pub fn embedded() -> &'static KeyboardArt {
        static ART: OnceLock<KeyboardArt> = OnceLock::new();
        ART.get_or_init(|| KeyboardArt::parse(KEYBOARD_ART))
    }

    fn parse(text: &str) -> KeyboardArt {
        let raw_lines: Vec<Vec<char>> = text.lines().map(|l| l.chars().collect()).collect();
        let width = raw_lines.iter().map(|l| l.len()).max().unwrap_or(0);
        let grid: Vec<Vec<char>> = raw_lines
            .into_iter()
            .map(|mut l| {
                l.resize(width, ' ');
                l
            })
            .collect();
        let height = grid.len();

        let mut instances: HashMap<String, Vec<KeyInst>> = HashMap::new();
        for r in 0..height {
            for c in 0..grid[r].len() {
                if grid[r][c] != TOP_LEFT {
                    continue;
                }
                let Some(bottom) =
                    (r + 1..height).find(|&rr| c < grid[rr].len() && grid[rr][c] == BOTTOM_LEFT)
                else {
                    continue;
                };
                let bottom_row = &grid[bottom];
                let Some(right) =
                    (c + 1..bottom_row.len()).find(|&cc| bottom_row[cc] == BOTTOM_RIGHT)
                else {
                    continue;
                };

                let label_row = &grid[r + 1];
                if right >= label_row.len() {
                    continue;
                }
                let inner = &label_row[c + 1..right];
                let mut start = 0;
                let mut end = inner.len();
                while start < end && (inner[start] == ' ' || inner[start] == VBAR) {
                    start += 1;
                }
                while end > start && (inner[end - 1] == ' ' || inner[end - 1] == VBAR) {
                    end -= 1;
                }
                let label: String = inner[start..end].iter().collect();
                let has_left = c + 1 < grid[r].len() && grid[r][c + 1] == TOP_LEFT_INNER;
                let has_right = right >= 1
                    && right - 1 < grid[r].len()
                    && grid[r][right - 1] == TOP_RIGHT_INNER;
                let label_col = if label.is_empty() {
                    c + 1
                } else {
                    c + 1 + start
                };

                instances.entry(label).or_default().push(KeyInst {
                    r,
                    c,
                    right,
                    bottom,
                    has_left,
                    has_right,
                    label_col,
                });
            }
        }

        KeyboardArt {
            width: width as u16,
            height: height as u16,
            grid,
            instances,
        }
    }

    fn apply_state(
        &self,
        caps: bool,
        shift: bool,
        overrides: &HashMap<String, String>,
    ) -> Vec<Vec<char>> {
        let mut rows = self.grid.clone();
        for (label, insts) in &self.instances {
            if let Some(replacement) = overrides.get(label) {
                for inst in insts {
                    write_label_override(&mut rows, inst, replacement);
                }
                continue;
            }
            let Some(letter) = is_single_letter(label) else {
                continue;
            };
            let ch = if caps != shift {
                letter.to_ascii_uppercase()
            } else {
                letter.to_ascii_lowercase()
            };
            for inst in insts {
                rows[inst.r + 1][inst.label_col] = ch;
            }
        }
        rows
    }
}

fn write_label_override(rows: &mut [Vec<char>], inst: &KeyInst, replacement: &str) {
    let label_row = &mut rows[inst.r + 1];
    let slot_start = inst.c + 1 + usize::from(inst.has_left);
    let slot_end = inst.right - usize::from(inst.has_right);
    if slot_end <= slot_start {
        return;
    }
    for cell in &mut label_row[slot_start..slot_end] {
        *cell = ' ';
    }
    // Left-align with one leading space so the override visually matches the
    // way " Enter      " is laid out in the source art.
    let leading = 1;
    for (i, ch) in replacement.chars().enumerate() {
        let pos = slot_start + leading + i;
        if pos >= slot_end {
            break;
        }
        label_row[pos] = ch;
    }
}

#[derive(Debug, Clone)]
struct PressCell {
    r: usize,
    c: usize,
    swapped: char,
    is_label: bool,
}

fn corner_cells(inst: &KeyInst) -> [(usize, usize); 4] {
    let r = inst.r;
    let c = inst.c;
    let right = inst.right;
    let bottom = inst.bottom;
    [(r, c), (r, right), (bottom, c), (bottom, right)]
}

fn compute_press_cells(state_grid: &[Vec<char>], inst: &KeyInst) -> Vec<PressCell> {
    let r = inst.r;
    let c = inst.c;
    let right = inst.right;
    let label_row = &state_grid[r + 1];
    let mut pressed_label: Vec<char> = label_row.clone();

    if inst.has_left && inst.has_right {
        pressed_label[c + 1] = ' ';
        pressed_label[right - 1] = ' ';
    } else if inst.has_left {
        pressed_label[c + 1..right - 1].copy_from_slice(&label_row[c + 2..right]);
        pressed_label[right - 1] = ' ';
    } else if inst.has_right {
        for i in (c + 2..right).rev() {
            pressed_label[i] = label_row[i - 1];
        }
        pressed_label[c + 1] = ' ';
    }

    let mut cells = Vec::new();
    if inst.has_left {
        cells.push(PressCell {
            r,
            c: c + 1,
            swapped: HBAR,
            is_label: false,
        });
        cells.push(PressCell {
            r: r + 2,
            c: c + 1,
            swapped: HBAR,
            is_label: false,
        });
    }
    if inst.has_right {
        cells.push(PressCell {
            r,
            c: right - 1,
            swapped: HBAR,
            is_label: false,
        });
        cells.push(PressCell {
            r: r + 2,
            c: right - 1,
            swapped: HBAR,
            is_label: false,
        });
    }
    for i in c..=right {
        let orig = label_row[i];
        let new = pressed_label[i];
        let is_label = new != ' ' && new != VBAR;
        if orig != new || is_label {
            cells.push(PressCell {
                r: r + 1,
                c: i,
                swapped: new,
                is_label,
            });
        }
    }
    cells
}

#[derive(Debug, Default, Clone)]
pub struct KeyboardState {
    pressed: HashMap<String, Instant>,
    wrong: HashMap<String, Instant>,
}

impl KeyboardState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn press(&mut self, label: impl Into<String>) {
        self.pressed
            .insert(label.into(), Instant::now() + FLASH_DURATION);
    }

    pub fn note_event(&mut self, ev: &KeyEvent) {
        if let Some(label) = key_to_label(ev) {
            self.press(label);
        }
        if ev.modifiers.contains(KeyModifiers::SHIFT) {
            self.press("Shift");
        }
        if ev.modifiers.contains(KeyModifiers::CONTROL) {
            self.press("Ctrl");
        }
        if ev.modifiers.contains(KeyModifiers::ALT) {
            self.press("Alt");
        }
    }

    pub fn mark_wrong(&mut self, ev: &KeyEvent) {
        if let Some(label) = key_to_label(ev) {
            self.wrong.insert(label, Instant::now() + FLASH_DURATION);
        }
    }

    pub fn tick(&mut self) {
        let now = Instant::now();
        self.pressed.retain(|_, deadline| *deadline > now);
        self.wrong.retain(|_, deadline| *deadline > now);
    }

    pub fn next_deadline(&self) -> Option<Instant> {
        self.pressed
            .values()
            .chain(self.wrong.values())
            .min()
            .copied()
    }

    pub fn has_active_flashes(&self) -> bool {
        self.next_deadline().is_some()
    }

    pub fn is_pressed(&self, label: &str) -> bool {
        self.pressed
            .get(label)
            .is_some_and(|d| *d > Instant::now())
    }
}

/// Split `area` into a display rect (top) and an optional keyboard rect (bottom).
/// When the terminal is at least keyboard-width, the display is clamped to
/// keyboard-width and centered horizontally regardless of `kb_visible`, so
/// successive screens stay in the same column.
pub fn split_with_keyboard(area: Rect, kb_visible: bool) -> (Rect, Option<Rect>) {
    let art = KeyboardArt::embedded();
    let wide_enough = area.width >= art.width;
    let (x, width) = if wide_enough {
        (area.x + (area.width - art.width) / 2, art.width)
    } else {
        (area.x, area.width)
    };
    let kb_fits = kb_visible && wide_enough && area.height > art.height;
    if !kb_fits {
        return (
            Rect {
                x,
                y: area.y,
                width,
                height: area.height,
            },
            None,
        );
    }
    let kb = Rect {
        x,
        y: area.y + area.height - art.height,
        width: art.width,
        height: art.height,
    };
    let display = Rect {
        x,
        y: area.y,
        width,
        height: area.height - art.height,
    };
    (display, Some(kb))
}

pub fn key_to_label(ev: &KeyEvent) -> Option<String> {
    match ev.code {
        KeyCode::Esc => Some("Esc".into()),
        KeyCode::Tab | KeyCode::BackTab => Some("Tab".into()),
        KeyCode::CapsLock => Some("Caps".into()),
        KeyCode::Enter => Some("Enter".into()),
        KeyCode::Backspace => Some("Backspace".into()),
        KeyCode::Char(' ') => Some("Space".into()),
        KeyCode::Char(c) => char_to_label(c),
        _ => None,
    }
}

fn char_to_label(c: char) -> Option<String> {
    let s: &str = match c {
        '`' | '~' => "`~",
        '1' | '!' => "1!",
        '2' | '@' => "2@",
        '3' | '#' => "3#",
        '4' | '$' => "4$",
        '5' | '%' => "5%",
        '6' | '^' => "6^",
        '7' | '&' => "7&",
        '8' | '*' => "8*",
        '9' | '(' => "9(",
        '0' | ')' => "0)",
        '-' | '_' => "-_",
        '=' | '+' => "=+",
        '[' | '{' => "[{",
        ']' | '}' => "]}",
        '\\' | '|' => "\\|",
        ';' | ':' => ";:",
        '\'' | '"' => "'\"",
        ',' | '<' => ",<",
        '.' | '>' => ".>",
        '/' | '?' => "/?",
        c if c.is_ascii_alphabetic() => return Some(c.to_ascii_uppercase().to_string()),
        c if (c as u32) >= 1 && (c as u32) <= 26 => {
            // Ctrl+letter often arrives as a control byte.
            return Some(((c as u8 - 1 + b'A') as char).to_string());
        }
        _ => return None,
    };
    Some(s.into())
}

pub struct KeyboardWidget<'a> {
    pub art: &'a KeyboardArt,
    pub state: &'a KeyboardState,
    pub overrides: HashMap<String, String>,
}

impl<'a> KeyboardWidget<'a> {
    pub fn new(state: &'a KeyboardState) -> Self {
        Self {
            art: KeyboardArt::embedded(),
            state,
            overrides: HashMap::new(),
        }
    }

    pub fn with_overrides(mut self, overrides: HashMap<String, String>) -> Self {
        self.overrides = overrides;
        self
    }
}

impl ThemedWidget for KeyboardWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer, theme: &Theme) {
        let art = self.art;
        if area.width < art.width || area.height < art.height {
            return;
        }
        let state_grid = art.apply_state(false, false, &self.overrides);

        let base_style = theme.prompt_untyped;
        for (r, row) in state_grid.iter().enumerate() {
            let s: String = row.iter().collect();
            buf.set_string(area.x, area.y + r as u16, &s, base_style);
        }

        let label_style = theme.prompt_current_correct;
        let corner_style = theme.prompt_current_correct;
        let wrong_style = theme.prompt_current_incorrect;
        let now = Instant::now();
        for (label, deadline) in self.state.pressed.iter() {
            if *deadline <= now {
                continue;
            }
            let Some(insts) = art.instances.get(label) else {
                continue;
            };
            let is_wrong = self.state.wrong.get(label).is_some_and(|d| *d > now);
            let (corner_s, label_s) = if is_wrong {
                (wrong_style, wrong_style)
            } else {
                (corner_style, label_style)
            };
            for inst in insts {
                for (br, bc) in corner_cells(inst) {
                    let ch = state_grid[br][bc];
                    let x = area.x + bc as u16;
                    let y = area.y + br as u16;
                    buf.set_string(x, y, ch.to_string(), corner_s);
                }
                for cell in compute_press_cells(&state_grid, inst) {
                    let x = area.x + cell.c as u16;
                    let y = area.y + cell.r as u16;
                    let style = if cell.is_label { label_s } else { base_style };
                    buf.set_string(x, y, cell.swapped.to_string(), style);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyEventKind;

    fn ke(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        }
    }

    #[test]
    fn embedded_art_parses() {
        let art = KeyboardArt::embedded();
        assert!(art.width > 100);
        assert!(art.height >= 20);
        // A representative letter, special, and pair are present.
        assert!(art.instances.contains_key("Q"));
        assert!(art.instances.contains_key("Backspace"));
        assert!(art.instances.contains_key("1!"));
        // Shift appears twice (left and right).
        assert_eq!(art.instances.get("Shift").map(|v| v.len()), Some(2));
    }

    #[test]
    fn key_to_label_basics() {
        assert_eq!(key_to_label(&ke(KeyCode::Esc)).as_deref(), Some("Esc"));
        assert_eq!(key_to_label(&ke(KeyCode::Char('a'))).as_deref(), Some("A"));
        assert_eq!(key_to_label(&ke(KeyCode::Char('!'))).as_deref(), Some("1!"));
        assert_eq!(
            key_to_label(&ke(KeyCode::Char(' '))).as_deref(),
            Some("Space")
        );
        assert_eq!(
            key_to_label(&ke(KeyCode::Char('|'))).as_deref(),
            Some("\\|")
        );
    }

    #[test]
    fn apply_state_lowercases_letters_keeps_pairs() {
        let art = KeyboardArt::embedded();
        let no_overrides: HashMap<String, String> = HashMap::new();
        let g = art.apply_state(false, false, &no_overrides);
        let q = &art.instances["Q"][0];
        assert_eq!(g[q.r + 1][q.label_col], 'q');
        let one = &art.instances["1!"][0];
        assert_eq!(g[one.r + 1][one.label_col], '1');
        assert_eq!(g[one.r + 1][one.label_col + 1], '!');

        let g_shift = art.apply_state(false, true, &no_overrides);
        assert_eq!(g_shift[q.r + 1][q.label_col], 'Q');
        // Pairs render both glyphs as in the art, regardless of shift state.
        assert_eq!(g_shift[one.r + 1][one.label_col], '1');
        assert_eq!(g_shift[one.r + 1][one.label_col + 1], '!');
    }

    #[test]
    fn apply_state_writes_overrides() {
        let art = KeyboardArt::embedded();
        let mut overrides: HashMap<String, String> = HashMap::new();
        overrides.insert("H".into(), "←H".into());
        let g = art.apply_state(false, false, &overrides);
        let h = &art.instances["H"][0];
        // The override is left-aligned with 1 leading space inside the slot.
        let row = &g[h.r + 1];
        let label: String = row[h.c + 1..h.right].iter().collect();
        assert!(label.contains('←'), "row was: {:?}", label);
        assert!(label.contains('H'), "row was: {:?}", label);
    }

    #[test]
    fn pressed_decays_after_flash() {
        let mut s = KeyboardState::new();
        s.press("Q");
        assert!(s.next_deadline().is_some());
        std::thread::sleep(FLASH_DURATION + Duration::from_millis(20));
        s.tick();
        assert!(s.next_deadline().is_none());
    }
}
