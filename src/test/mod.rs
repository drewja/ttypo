pub mod results;

use crate::content::Content;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use std::fmt;
use std::ops::Range;
use std::sync::Arc;
use std::time::Instant;

/// Returns true if a character is printable ASCII (0x20-0x7E).
pub fn is_typeable(c: char) -> bool {
    c.is_ascii() && !c.is_ascii_control()
}

/// Returns the typeable portion of `text` as a `Box<str>`.
fn target_text(text: &str, ascii: bool) -> Box<str> {
    if ascii {
        text.chars()
            .filter(|c| is_typeable(*c))
            .collect::<String>()
            .into_boxed_str()
    } else {
        text.to_string().into_boxed_str()
    }
}

#[derive(Clone)]
pub struct TestEvent {
    pub time: Instant,
    pub key: KeyEvent,
    pub correct: Option<bool>,
    /// Target character expected at the cursor position when this press
    /// landed. `None` for non-char events (space/enter commits, backspace,
    /// Ctrl-h/Ctrl-w) and for presses past the end of the word's target.
    pub target: Option<char>,
}

pub fn is_missed_word_event(event: &TestEvent) -> bool {
    event.correct != Some(true)
}

impl fmt::Debug for TestEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TestEvent")
            .field("time", &String::from("Instant { ... }"))
            .field("key", &self.key)
            .field("target", &self.target)
            .finish()
    }
}

#[derive(Debug, Clone)]
pub struct TestWord {
    /// Byte range into the owning `Test::content.as_str()`.
    pub range: Range<u32>,
    /// Cached target string (ascii-filtered when `ascii`, else equal to
    /// the word's display text). Computed once at construction.
    pub target: Box<str>,
    pub progress: String,
    pub events: Vec<TestEvent>,
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
    pub content: Arc<Content>,
    pub words: Vec<TestWord>,
    pub current_word: usize,
    pub complete: bool,
    pub backtracking_enabled: bool,
    pub sudden_death_enabled: bool,
    pub backspace_enabled: bool,
    /// When true, non-typeable characters are skipped during typing.
    pub ascii: bool,
    pub start_time: Option<Instant>,
    /// Label describing the source of the test contents (language name,
    /// filename, "stdin", or "practice").
    pub source: String,
}

impl Test {
    pub fn new(
        content: Arc<Content>,
        backtracking_enabled: bool,
        sudden_death_enabled: bool,
        backspace_enabled: bool,
        ascii: bool,
        source: String,
    ) -> Self {
        let text = content.as_str();
        let words: Vec<TestWord> = content
            .word_ranges
            .iter()
            .map(|r| {
                let word_text = &text[r.start as usize..r.end as usize];
                TestWord {
                    range: r.clone(),
                    target: target_text(word_text, ascii),
                    progress: String::new(),
                    events: Vec::new(),
                }
            })
            .collect();

        let mut test = Self {
            content,
            words,
            current_word: 0,
            complete: false,
            backtracking_enabled,
            sudden_death_enabled,
            backspace_enabled,
            ascii,
            start_time: None,
            source,
        };
        test.skip_non_typeable_words();
        test
    }

    pub fn word_text(&self, idx: usize) -> &str {
        let r = &self.words[idx].range;
        &self.content.as_str()[r.start as usize..r.end as usize]
    }

    pub fn lines(&self) -> &[DisplayLine] {
        &self.content.lines
    }

    /// Resume at `word_index`, synthesizing completed progress for every word
    /// in `[0, word_index)` so they render in the "correct" color via the
    /// existing draw path. Clamped to the last word if out of range.
    pub fn resume_at(&mut self, word_index: usize) {
        if self.words.is_empty() {
            return;
        }
        let target_index = word_index.min(self.words.len() - 1);
        for word in self.words.iter_mut().take(target_index) {
            word.progress = word.target.to_string();
        }
        self.current_word = target_index;
        self.skip_non_typeable_words();
    }

    pub fn elapsed_secs(&self) -> f64 {
        self.start_time
            .map(|t| t.elapsed().as_secs_f64())
            .unwrap_or(0.0)
    }

    /// Rolling WPM over the last ~10 keypresses. Uses the 11 most recent
    /// event timestamps to get up to 10 interval durations, averages them for
    /// a CPS, then scales to WPM (60 sec / 5 chars per word = *12).
    pub fn live_wpm(&self) -> f64 {
        const WINDOW: usize = 10;
        let mut times: Vec<Instant> = self
            .words
            .iter()
            .flat_map(|w| w.events.iter().map(|e| e.time))
            .collect();
        if times.len() < 2 {
            return 0.0;
        }
        times.sort_unstable();
        let start = times.len().saturating_sub(WINDOW + 1);
        let window = &times[start..];
        let span = window
            .last()
            .unwrap()
            .duration_since(*window.first().unwrap())
            .as_secs_f64();
        if span <= 0.0 {
            return 0.0;
        }
        let intervals = (window.len() - 1) as f64;
        (intervals / span) * 12.0
    }

    pub fn progress(&self) -> (usize, usize) {
        (self.current_word, self.words.len())
    }

    /// True if the user has registered any keypress in this test. Used to
    /// suppress the (empty) results screen when the user aborts before
    /// typing anything.
    pub fn has_events(&self) -> bool {
        self.words.iter().any(|w| !w.events.is_empty())
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        if key.kind != KeyEventKind::Press {
            return false;
        }

        if self.start_time.is_none() {
            self.start_time = Some(Instant::now());
        }

        let mut wrong = false;
        let word = &mut self.words[self.current_word];
        match key.code {
            KeyCode::Char(' ') | KeyCode::Enter => {
                if word.target.chars().nth(word.progress.len()) == Some(' ') {
                    word.progress.push(' ');
                    word.events.push(TestEvent {
                        time: Instant::now(),
                        correct: Some(true),
                        key,
                        target: None,
                    });
                } else if !word.progress.is_empty() || word.target.is_empty() {
                    let correct = word.progress.as_str() == &*word.target;
                    if !correct {
                        wrong = true;
                    }
                    if self.sudden_death_enabled && !correct {
                        self.reset();
                    } else {
                        word.events.push(TestEvent {
                            time: Instant::now(),
                            correct: Some(correct),
                            key,
                            target: None,
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
                    let correct = !word.target.starts_with(word.progress.as_str());
                    word.events.push(TestEvent {
                        time: Instant::now(),
                        correct: Some(correct),
                        key,
                        target: None,
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
                    target: None,
                });
                word.progress.clear();
            }
            KeyCode::Char(c) => {
                let target_ch = word.target.chars().nth(word.progress.len());
                word.progress.push(c);
                let correct = word.target.starts_with(word.progress.as_str());
                if !correct {
                    wrong = true;
                }
                if self.sudden_death_enabled && !correct {
                    self.reset();
                } else {
                    word.events.push(TestEvent {
                        time: Instant::now(),
                        correct: Some(correct),
                        key,
                        target: target_ch,
                    });
                    // Complete the last word once it has reached its target
                    // length, regardless of correctness. Mirrors the implicit
                    // "commit on space" used for non-last words, which the
                    // last word can't do because there's no following word.
                    if word.progress.chars().count() >= word.target.chars().count()
                        && self.current_word == self.words.len() - 1
                    {
                        self.complete = true;
                        self.current_word = 0;
                    }
                }
            }
            _ => {}
        };
        wrong
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
            if !self.words[self.current_word].target.is_empty() {
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
pub(crate) fn flat_content(words: &[&str]) -> Arc<Content> {
    Arc::new(Content::from_word_list(
        words.iter().copied(),
        String::new(),
    ))
}

/// Layout-aware test content: caller hands in an explicit buffer with `\n`
/// between lines and ` ` between words; tokenization (ranges + DisplayLines)
/// is delegated to `Content::from_text` rather than recomputed alongside it.
#[cfg(test)]
pub(crate) fn layout_content(buf: &str) -> Arc<Content> {
    Arc::new(Content::from_text(buf.to_string(), String::new()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyEventState;
    use std::time::Duration;

    fn make_test(words: &[&str], lines: Vec<DisplayLine>, ascii: bool) -> Test {
        debug_assert!(
            lines.is_empty(),
            "make_test is for flat word lists; use make_layout_test for layout-aware tests",
        );
        let _ = lines;
        Test::new(flat_content(words), true, false, true, ascii, String::new())
    }

    fn make_layout_test(buf: &str, ascii: bool) -> Test {
        Test::new(layout_content(buf), true, false, true, ascii, String::new())
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
        let test = make_layout_test("a b\nc d", false);
        assert_eq!(test.lines().len(), 2);
        assert_eq!(test.lines()[0].word_start, 0);
        assert_eq!(test.lines()[1].word_start, 2);
        assert_eq!(test.words.len(), 4);
    }

    #[test]
    fn reset_preserves_lines() {
        let mut test = make_layout_test("a\nb c", false);
        test.words[0].progress = "a".to_string();
        test.current_word = 1;
        test.words[1].progress = "x".to_string();

        test.reset();

        assert_eq!(test.current_word, 0);
        assert!(!test.complete);
        assert!(test.words.iter().all(|w| w.progress.is_empty()));
        assert_eq!(test.lines().len(), 2);
    }

    #[test]
    fn target_text_without_ascii() {
        assert_eq!(&*target_text("café", false), "café");
    }

    #[test]
    fn target_text_with_ascii() {
        assert_eq!(&*target_text("café", true), "caf");
        assert_eq!(&*target_text("hello—world", true), "helloworld");
        assert_eq!(&*target_text("“quoted”", true), "quoted");
    }

    #[test]
    fn last_word_completes_on_mistype_at_full_length() {
        let mut test = make_test(&["hi", "world"], Vec::new(), false);
        for c in "hi".chars() {
            test.handle_key(press(c));
        }
        test.handle_key(press_space());
        for c in "wxrld".chars() {
            test.handle_key(press(c));
        }
        assert!(test.complete);
    }

    #[test]
    fn last_word_does_not_complete_before_full_length() {
        let mut test = make_test(&["hi", "world"], Vec::new(), false);
        for c in "hi".chars() {
            test.handle_key(press(c));
        }
        test.handle_key(press_space());
        for c in "wor".chars() {
            test.handle_key(press(c));
        }
        assert!(!test.complete);
    }

    #[test]
    fn ascii_skips_unicode_in_typing() {
        let mut test = make_test(&["café"], Vec::new(), true);
        for c in "caf".chars() {
            test.handle_key(press(c));
        }
        assert!(test.complete);
    }

    #[test]
    fn ascii_space_advances_past_unicode_word() {
        let mut test = make_test(&["café", "ok"], Vec::new(), true);
        for c in "caf".chars() {
            test.handle_key(press(c));
        }
        test.handle_key(press_space());
        assert_eq!(test.current_word, 1);
    }

    #[test]
    fn ascii_auto_skips_all_unicode_word() {
        let test = make_test(&["——", "ok"], Vec::new(), true);
        // entirely non-typeable word is auto-skipped at construction
        assert_eq!(test.current_word, 1);
    }

    #[test]
    fn ascii_auto_skips_chain_of_unicode_words() {
        let test = make_test(&["—", "éé", "ok"], Vec::new(), true);
        // both non-typeable words skipped at construction
        assert_eq!(test.current_word, 2);
    }

    #[test]
    fn ascii_auto_skips_after_space() {
        let mut test = make_test(&["hi", "—", "ok"], Vec::new(), true);
        for c in "hi".chars() {
            test.handle_key(press(c));
        }
        test.handle_key(press_space());
        // skipped past the non-typeable word to "ok"
        assert_eq!(test.current_word, 2);
    }

    #[test]
    fn ascii_all_non_typeable_completes() {
        let test = make_test(&["—", "é"], Vec::new(), true);
        assert!(test.complete);
    }

    #[test]
    fn without_ascii_unicode_must_be_typed() {
        let mut test = make_test(&["café"], Vec::new(), false);
        for c in "caf".chars() {
            test.handle_key(press(c));
        }
        assert!(!test.complete);
    }

    #[test]
    fn without_ascii_no_auto_skip() {
        let test = make_test(&["—", "ok"], Vec::new(), false);
        // without ascii, no auto-skipping
        assert_eq!(test.current_word, 0);
    }

    #[test]
    fn resume_at_fills_progress_for_prefix_words() {
        let mut test = make_test(&["alpha", "beta", "gamma", "delta"], Vec::new(), false);
        test.resume_at(2);
        assert_eq!(test.words[0].progress, "alpha");
        assert_eq!(test.words[1].progress, "beta");
        // current word onward stays empty
        assert_eq!(test.words[2].progress, "");
        assert_eq!(test.words[3].progress, "");
        assert_eq!(test.current_word, 2);
    }

    #[test]
    fn resume_at_clamps_past_end() {
        let mut test = make_test(&["a", "b", "c"], Vec::new(), false);
        test.resume_at(999);
        assert_eq!(test.current_word, 2);
        assert_eq!(test.words[0].progress, "a");
        assert_eq!(test.words[1].progress, "b");
    }

    #[test]
    fn resume_at_zero_is_noop() {
        let mut test = make_test(&["a", "b"], Vec::new(), false);
        test.resume_at(0);
        assert_eq!(test.current_word, 0);
        assert!(test.words.iter().all(|w| w.progress.is_empty()));
    }

    fn push_event_at(word: &mut TestWord, time: Instant) {
        word.events.push(TestEvent {
            time,
            key: KeyEvent {
                code: KeyCode::Char('x'),
                modifiers: KeyModifiers::NONE,
                kind: KeyEventKind::Press,
                state: crossterm::event::KeyEventState::NONE,
            },
            correct: Some(true),
            target: None,
        });
    }

    #[test]
    fn live_wpm_rolls_over_last_10_keypresses() {
        // 11 events 100 ms apart → 10 intervals of 0.1 s → cps = 10 → wpm = 120.
        let mut test = make_test(&["aaaaaaaaaaaaaa"], Vec::new(), false);
        let base = Instant::now();
        for i in 0..11 {
            push_event_at(&mut test.words[0], base + Duration::from_millis(100 * i));
        }
        let wpm = test.live_wpm();
        assert!((wpm - 120.0).abs() < 1e-6, "expected 120 wpm, got {}", wpm);
    }

    #[test]
    fn live_wpm_ignores_older_events_outside_window() {
        // One slow early event, then 11 fast events: the slow one must be
        // outside the rolling window and have no effect.
        let mut test = make_test(&["aaaaaaaaaaaaaaa"], Vec::new(), false);
        let base = Instant::now();
        push_event_at(&mut test.words[0], base); // ancient event
        for i in 0..11 {
            push_event_at(
                &mut test.words[0],
                base + Duration::from_secs(60) + Duration::from_millis(100 * i),
            );
        }
        let wpm = test.live_wpm();
        assert!((wpm - 120.0).abs() < 1e-6, "expected 120 wpm, got {}", wpm);
    }

    #[test]
    fn live_wpm_excludes_resumed_prefix() {
        // Pre-filled resumed words carry no events, so they can't contribute
        // to the rolling window regardless of their progress strings.
        let mut test = make_test(&["aaaaa", "bbbbb"], Vec::new(), false);
        test.resume_at(1);
        test.words[1].progress = "bbbbb".into();
        let base = Instant::now();
        for i in 0..11 {
            push_event_at(&mut test.words[1], base + Duration::from_millis(100 * i));
        }
        let wpm = test.live_wpm();
        assert!((wpm - 120.0).abs() < 1e-6, "expected 120 wpm, got {}", wpm);
    }
}
