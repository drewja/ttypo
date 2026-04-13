pub mod results;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use std::fmt;
use std::time::Instant;

/// Returns true if a character is printable ASCII (0x20-0x7E).
pub fn is_typeable(c: char) -> bool {
    c.is_ascii() && !c.is_ascii_control()
}

/// Returns the typeable portion of `text`.
/// When `ascii` is false the full text is returned unchanged.
fn target_text(text: &str, ascii: bool) -> String {
    if ascii {
        text.chars().filter(|c| is_typeable(*c)).collect()
    } else {
        text.to_string()
    }
}

#[derive(Clone)]
pub struct TestEvent {
    pub time: Instant,
    pub key: KeyEvent,
    pub correct: Option<bool>,
}

pub fn is_missed_word_event(event: &TestEvent) -> bool {
    event.correct != Some(true)
}

impl fmt::Debug for TestEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TestEvent")
            .field("time", &String::from("Instant { ... }"))
            .field("key", &self.key)
            .finish()
    }
}

#[derive(Debug, Clone)]
pub struct TestWord {
    pub text: String,
    pub progress: String,
    pub events: Vec<TestEvent>,
}

impl From<String> for TestWord {
    fn from(string: String) -> Self {
        TestWord {
            text: string,
            progress: String::new(),
            events: Vec::new(),
        }
    }
}

impl From<&str> for TestWord {
    fn from(string: &str) -> Self {
        Self::from(string.to_string())
    }
}

/// A line of the original file, used in raw mode for display.
#[derive(Debug, Clone)]
pub struct DisplayLine {
    /// Leading whitespace (tabs expanded to 4 spaces).
    pub indent: String,
    /// Index of the first word on this line in `Test::words`.
    pub word_start: usize,
    /// Number of words on this line (0 for empty/whitespace-only lines).
    pub word_count: usize,
}

#[derive(Debug, Clone)]
pub struct Test {
    pub words: Vec<TestWord>,
    pub current_word: usize,
    pub complete: bool,
    pub backtracking_enabled: bool,
    pub sudden_death_enabled: bool,
    pub backspace_enabled: bool,
    /// Original line layout for raw/file mode (empty = word-wrap mode).
    pub lines: Vec<DisplayLine>,
    /// When true, non-typeable characters are skipped during typing.
    pub ascii: bool,
    pub start_time: Option<Instant>,
}

impl Test {
    pub fn new(
        words: Vec<String>,
        backtracking_enabled: bool,
        sudden_death_enabled: bool,
        backspace_enabled: bool,
        lines: Vec<DisplayLine>,
        ascii: bool,
    ) -> Self {
        let mut test = Self {
            words: words.into_iter().map(TestWord::from).collect(),
            current_word: 0,
            complete: false,
            backtracking_enabled,
            sudden_death_enabled,
            backspace_enabled,
            lines,
            ascii,
            start_time: None,
        };
        test.skip_non_typeable_words();
        test
    }

    pub fn elapsed_secs(&self) -> f64 {
        self.start_time
            .map(|t| t.elapsed().as_secs_f64())
            .unwrap_or(0.0)
    }

    pub fn live_wpm(&self) -> f64 {
        let elapsed = self.elapsed_secs();
        if elapsed < 0.5 {
            return 0.0;
        }
        let chars_typed: usize = self.words[..self.current_word]
            .iter()
            .map(|w| w.progress.len())
            .sum::<usize>()
            + self.words[self.current_word].progress.len();
        // Standard: 1 word = 5 characters
        (chars_typed as f64 / 5.0) / (elapsed / 60.0)
    }

    pub fn progress(&self) -> (usize, usize) {
        (self.current_word, self.words.len())
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if key.kind != KeyEventKind::Press {
            return;
        }

        if self.start_time.is_none() {
            self.start_time = Some(Instant::now());
        }

        let ascii = self.ascii;
        let word = &mut self.words[self.current_word];
        let target = target_text(&word.text, ascii);
        match key.code {
            KeyCode::Char(' ') | KeyCode::Enter => {
                if target.chars().nth(word.progress.len()) == Some(' ') {
                    word.progress.push(' ');
                    word.events.push(TestEvent {
                        time: Instant::now(),
                        correct: Some(true),
                        key,
                    })
                } else if !word.progress.is_empty() || target.is_empty() {
                    let correct = target == word.progress;
                    if self.sudden_death_enabled && !correct {
                        self.reset();
                    } else {
                        word.events.push(TestEvent {
                            time: Instant::now(),
                            correct: Some(correct),
                            key,
                        });
                        self.next_word();
                        self.skip_non_typeable_words();
                    }
                }
            }
            KeyCode::Backspace => {
                if word.progress.is_empty() && self.backtracking_enabled && self.backspace_enabled {
                    self.last_word();
                } else if self.backspace_enabled {
                    word.events.push(TestEvent {
                        time: Instant::now(),
                        correct: Some(!target.starts_with(&word.progress[..])),
                        key,
                    });
                    word.progress.pop();
                }
            }
            // CTRL-BackSpace and CTRL-W
            KeyCode::Char('h') | KeyCode::Char('w')
                if key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                if self.words[self.current_word].progress.is_empty() {
                    self.last_word();
                }

                let word = &mut self.words[self.current_word];

                word.events.push(TestEvent {
                    time: Instant::now(),
                    correct: None,
                    key,
                });
                word.progress.clear();
            }
            KeyCode::Char(c) => {
                word.progress.push(c);
                let correct = target.starts_with(&word.progress[..]);
                if self.sudden_death_enabled && !correct {
                    self.reset();
                } else {
                    word.events.push(TestEvent {
                        time: Instant::now(),
                        correct: Some(correct),
                        key,
                    });
                    if word.progress == target && self.current_word == self.words.len() - 1 {
                        self.complete = true;
                        self.current_word = 0;
                    }
                }
            }
            _ => {}
        };
    }

    fn last_word(&mut self) {
        if self.current_word != 0 {
            self.current_word -= 1;
        }
    }

    fn next_word(&mut self) {
        if self.current_word == self.words.len() - 1 {
            self.complete = true;
            self.current_word = 0;
        } else {
            self.current_word += 1;
        }
    }

    fn reset(&mut self) {
        self.words.iter_mut().for_each(|word: &mut TestWord| {
            word.progress.clear();
            word.events.clear();
        });
        self.current_word = 0;
        self.complete = false;
        self.start_time = None;
        self.skip_non_typeable_words();
    }

    fn skip_non_typeable_words(&mut self) {
        if !self.ascii || self.complete {
            return;
        }
        loop {
            let t = target_text(&self.words[self.current_word].text, true);
            if !t.is_empty() {
                break;
            }
            self.next_word();
            if self.complete {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyEventState;

    fn make_test(words: &[&str], lines: Vec<DisplayLine>, ascii: bool) -> Test {
        Test::new(
            words.iter().map(|s| s.to_string()).collect(),
            true,
            false,
            true,
            lines,
            ascii,
        )
    }

    fn press(c: char) -> KeyEvent {
        KeyEvent {
            code: KeyCode::Char(c),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    fn press_space() -> KeyEvent {
        KeyEvent {
            code: KeyCode::Char(' '),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    #[test]
    fn new_preserves_lines() {
        let lines = vec![
            DisplayLine {
                indent: String::new(),
                word_start: 0,
                word_count: 2,
            },
            DisplayLine {
                indent: String::new(),
                word_start: 2,
                word_count: 2,
            },
        ];
        let test = make_test(&["a", "b", "c", "d"], lines.clone(), false);
        assert_eq!(test.lines.len(), 2);
        assert_eq!(test.lines[0].word_start, 0);
        assert_eq!(test.lines[1].word_start, 2);
        assert_eq!(test.words.len(), 4);
    }

    #[test]
    fn new_empty_lines_for_word_mode() {
        let test = make_test(&["hello", "world"], Vec::new(), false);
        assert!(test.lines.is_empty());
    }

    #[test]
    fn reset_preserves_lines() {
        let lines = vec![
            DisplayLine {
                indent: String::new(),
                word_start: 0,
                word_count: 1,
            },
            DisplayLine {
                indent: String::new(),
                word_start: 1,
                word_count: 2,
            },
        ];
        let mut test = make_test(&["a", "b", "c"], lines, false);
        test.words[0].progress = "a".to_string();
        test.current_word = 1;
        test.words[1].progress = "x".to_string();

        test.reset();

        assert_eq!(test.current_word, 0);
        assert!(!test.complete);
        assert!(test.words.iter().all(|w| w.progress.is_empty()));
        assert_eq!(test.lines.len(), 2);
    }

    #[test]
    fn target_text_without_ascii() {
        assert_eq!(target_text("caf\u{00e9}", false), "caf\u{00e9}");
    }

    #[test]
    fn target_text_with_ascii() {
        assert_eq!(target_text("caf\u{00e9}", true), "caf");
        assert_eq!(target_text("hello\u{2014}world", true), "helloworld");
        assert_eq!(target_text("\u{201c}quoted\u{201d}", true), "quoted");
    }

    #[test]
    fn ascii_skips_unicode_in_typing() {
        let mut test = make_test(&["caf\u{00e9}"], Vec::new(), true);
        for c in "caf".chars() {
            test.handle_key(press(c));
        }
        assert!(test.complete);
    }

    #[test]
    fn ascii_space_advances_past_unicode_word() {
        let mut test = make_test(&["caf\u{00e9}", "ok"], Vec::new(), true);
        for c in "caf".chars() {
            test.handle_key(press(c));
        }
        test.handle_key(press_space());
        assert_eq!(test.current_word, 1);
    }

    #[test]
    fn ascii_auto_skips_all_unicode_word() {
        let test = make_test(&["\u{2014}\u{2014}", "ok"], Vec::new(), true);
        // entirely non-typeable word is auto-skipped at construction
        assert_eq!(test.current_word, 1);
    }

    #[test]
    fn ascii_auto_skips_chain_of_unicode_words() {
        let test = make_test(&["\u{2014}", "\u{00e9}\u{00e9}", "ok"], Vec::new(), true);
        // both non-typeable words skipped at construction
        assert_eq!(test.current_word, 2);
    }

    #[test]
    fn ascii_auto_skips_after_space() {
        let mut test = make_test(&["hi", "\u{2014}", "ok"], Vec::new(), true);
        for c in "hi".chars() {
            test.handle_key(press(c));
        }
        test.handle_key(press_space());
        // skipped past the non-typeable word to "ok"
        assert_eq!(test.current_word, 2);
    }

    #[test]
    fn ascii_all_non_typeable_completes() {
        let test = make_test(&["\u{2014}", "\u{00e9}"], Vec::new(), true);
        assert!(test.complete);
    }

    #[test]
    fn without_ascii_unicode_must_be_typed() {
        let mut test = make_test(&["caf\u{00e9}"], Vec::new(), false);
        for c in "caf".chars() {
            test.handle_key(press(c));
        }
        assert!(!test.complete);
    }

    #[test]
    fn without_ascii_no_auto_skip() {
        let test = make_test(&["\u{2014}", "ok"], Vec::new(), false);
        // without ascii, no auto-skipping
        assert_eq!(test.current_word, 0);
    }
}
