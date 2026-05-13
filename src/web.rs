//! WASM driver for ttypo.
//!
//! Wires the shared Title / Test / Results state machines into a
//! ratzilla DOM-rendered terminal in the browser. Mirrors the inner
//! event loop of `main.rs` but in callback form (no blocking I/O).

use std::cell::RefCell;
use std::num::NonZeroUsize;
use std::rc::Rc;
use std::sync::Arc;

use rand::seq::SliceRandom;
use ratzilla::WebRenderer;
use ratzilla::backend::dom::DomBackend;
use ratzilla::ratatui::{Frame, Terminal, layout::Rect};
use wasm_bindgen::prelude::*;

use crate::config::Config;
use crate::content::Content;
use crate::key::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crate::keyboard::{KeyboardState, KeyboardWidget, split_with_keyboard};
use crate::resources::Resources;
use crate::test::{Test, results::Results};
use crate::title::{self, Outcome as TitleOutcome, Title};

const DEFAULT_LANGUAGE: &str = "english200";
const DEFAULT_WORDS: usize = 50;

#[wasm_bindgen(start)]
pub fn start() -> Result<(), JsValue> {
    console_error_panic_hook::set_once();
    let backend = DomBackend::new().map_err(|e| JsValue::from_str(&format!("{e:?}")))?;
    let terminal = Terminal::new(backend).map_err(|e| JsValue::from_str(&e.to_string()))?;

    let session = Rc::new(RefCell::new(Session::new()));

    terminal.on_key_event({
        let session = session.clone();
        move |key: ratzilla::event::KeyEvent| {
            session.borrow_mut().handle_key(&convert_key(key));
        }
    });

    terminal.draw_web({
        let session = session.clone();
        move |frame: &mut Frame| {
            session.borrow_mut().render(frame);
        }
    });

    Ok(())
}

struct Session {
    languages: Vec<String>,
    config: Config,
    kb: KeyboardState,
    kb_visible: bool,
    settings: Settings,
    screen: Screen,
}

#[derive(Clone)]
struct Settings {
    language: String,
    words: NonZeroUsize,
    sudden_death: bool,
    no_backtrack: bool,
    no_backspace: bool,
    ascii: bool,
}

enum Screen {
    Title(Title),
    Test(Test),
    Results(Results),
}

impl Session {
    fn new() -> Self {
        let languages = available_languages();
        let language = if languages.iter().any(|l| l == DEFAULT_LANGUAGE) {
            DEFAULT_LANGUAGE.to_owned()
        } else {
            languages
                .first()
                .cloned()
                .unwrap_or_else(|| DEFAULT_LANGUAGE.to_owned())
        };
        let settings = Settings {
            language,
            words: NonZeroUsize::new(DEFAULT_WORDS).unwrap(),
            sudden_death: false,
            no_backtrack: false,
            no_backspace: false,
            ascii: true,
        };
        let title = build_title(&settings, &languages);
        Self {
            languages,
            config: Config::default(),
            kb: KeyboardState::new(),
            kb_visible: true,
            settings,
            screen: Screen::Title(title),
        }
    }

    fn handle_key(&mut self, ev: &KeyEvent) {
        if ev.kind == KeyEventKind::Press {
            self.kb.note_event(ev);
        }
        match &mut self.screen {
            Screen::Title(title) => {
                if let Some(outcome) = title.handle_key(ev, &mut self.kb_visible) {
                    match outcome {
                        TitleOutcome::Quit => {
                            // No process exit in the browser; reset to a
                            // fresh title screen.
                            let title = build_title(&self.settings, &self.languages);
                            self.screen = Screen::Title(title);
                        }
                        TitleOutcome::Start => {
                            self.settings.language = title.language.clone();
                            self.settings.words = title.words;
                            self.settings.sudden_death = title.sudden_death;
                            self.settings.no_backtrack = title.no_backtrack;
                            self.settings.no_backspace = title.no_backspace;
                            self.settings.ascii = title.ascii;
                            if let Some(test) = build_test(&self.settings) {
                                self.screen = Screen::Test(test);
                            }
                        }
                    }
                }
            }
            Screen::Test(test) => {
                if test.handle_key(*ev) {
                    self.kb.mark_wrong(ev);
                }
                if test.complete {
                    let results = Results::from(&*test);
                    self.screen = Screen::Results(results);
                }
            }
            Screen::Results(results) => {
                if ev.kind != KeyEventKind::Press {
                    return;
                }
                match ev.code {
                    KeyCode::Esc | KeyCode::Char('m') | KeyCode::Char('M') => {
                        let title = build_title(&self.settings, &self.languages);
                        self.screen = Screen::Title(title);
                    }
                    KeyCode::Char('r') | KeyCode::Char('R') => {
                        if let Some(test) = build_test(&self.settings) {
                            self.screen = Screen::Test(test);
                        }
                    }
                    KeyCode::Char('p') | KeyCode::Char('P') => {
                        if results.missed_words.is_empty() {
                            return;
                        }
                        let mut practice_words: Vec<String> = results
                            .missed_words
                            .iter()
                            .flat_map(|(w, _)| std::iter::repeat_n(w.clone(), 5))
                            .collect();
                        practice_words.shuffle(&mut rand::rng());
                        let content = Arc::new(Content::from_word_list(
                            practice_words,
                            "practice".to_string(),
                        ));
                        let test = Test::new(
                            content,
                            !self.settings.no_backtrack,
                            self.settings.sudden_death,
                            !self.settings.no_backspace,
                            self.settings.ascii,
                            "practice".to_string(),
                        );
                        self.screen = Screen::Test(test);
                    }
                    _ => {}
                }
            }
        }
    }

    fn render(&mut self, frame: &mut Frame) {
        // Tick keyboard flash decay each frame.
        self.kb.tick();
        let area = frame.area();
        match &self.screen {
            Screen::Title(title) => {
                title.render(frame, &self.config.theme, &self.kb, self.kb_visible);
            }
            Screen::Test(test) => {
                let (display, kb_rect) = split_with_keyboard(area, self.kb_visible);
                let test_area = if !test.lines().is_empty() {
                    Rect {
                        x: area.x,
                        width: area.width,
                        ..display
                    }
                } else {
                    display
                };
                frame.render_widget(self.config.theme.apply_to(test), test_area);
                if let Some(r) = kb_rect {
                    frame.render_widget(
                        self.config.theme.apply_to(KeyboardWidget::new(&self.kb)),
                        r,
                    );
                }
            }
            Screen::Results(results) => {
                // Hide the keyboard on the Results screen, matching native.
                let (display, _) = split_with_keyboard(area, false);
                frame.render_widget(self.config.theme.apply_to(results), display);
            }
        }
    }
}

fn available_languages() -> Vec<String> {
    let mut langs: Vec<String> = Resources::iter()
        .filter_map(|name| name.strip_prefix("language/").map(ToOwned::to_owned))
        .collect();
    langs.sort();
    langs
}

fn build_title(settings: &Settings, languages: &[String]) -> Title {
    Title::new(
        settings.language.clone(),
        settings.words,
        settings.sudden_death,
        settings.no_backtrack,
        settings.no_backspace,
        settings.ascii,
        languages.to_vec(),
    )
}

fn build_test(settings: &Settings) -> Option<Test> {
    let bytes = Resources::get(&format!("language/{}", settings.language))?
        .data
        .into_owned();
    let mut rng = rand::rng();
    let mut language_words: Vec<&str> = std::str::from_utf8(&bytes).ok()?.lines().collect();
    language_words.shuffle(&mut rng);
    let mut words: Vec<String> = language_words
        .into_iter()
        .cycle()
        .take(settings.words.get())
        .map(ToOwned::to_owned)
        .collect();
    words.shuffle(&mut rng);
    let content = Arc::new(Content::from_word_list(words, settings.language.clone()));
    Some(Test::new(
        content,
        !settings.no_backtrack,
        settings.sudden_death,
        !settings.no_backspace,
        settings.ascii,
        settings.language.clone(),
    ))
}

fn convert_key(rz: ratzilla::event::KeyEvent) -> KeyEvent {
    use ratzilla::event::KeyCode as RzCode;
    let code = match rz.code {
        RzCode::Char(c) => KeyCode::Char(c),
        RzCode::Backspace => KeyCode::Backspace,
        RzCode::Enter => KeyCode::Enter,
        RzCode::Left => KeyCode::Left,
        RzCode::Right => KeyCode::Right,
        RzCode::Up => KeyCode::Up,
        RzCode::Down => KeyCode::Down,
        RzCode::Home => KeyCode::Home,
        RzCode::End => KeyCode::End,
        RzCode::PageUp => KeyCode::PageUp,
        RzCode::PageDown => KeyCode::PageDown,
        RzCode::Tab => KeyCode::Tab,
        RzCode::Delete => KeyCode::Delete,
        RzCode::F(n) => KeyCode::F(n),
        RzCode::Esc => KeyCode::Esc,
        RzCode::Unidentified => KeyCode::Null,
    };
    let mut modifiers = KeyModifiers::empty();
    if rz.ctrl {
        modifiers |= KeyModifiers::CONTROL;
    }
    if rz.alt {
        modifiers |= KeyModifiers::ALT;
    }
    if rz.shift {
        modifiers |= KeyModifiers::SHIFT;
    }
    KeyEvent {
        code,
        modifiers,
        kind: KeyEventKind::Press,
    }
}

// Silence the title module dead-code lint when the wasm target doesn't pull
// in the native `title::run` function.
#[allow(dead_code)]
const _USE_TITLE: fn() = || {
    let _ = title::Outcome::Start;
};
