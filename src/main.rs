mod config;
mod content;
mod keyboard;
mod progress;
mod resume_prompt;
mod test;
mod title;
mod ui;

use config::Config;
use content::Content;
use keyboard::{KeyboardState, KeyboardWidget, split_with_keyboard};
use progress::ProgressStore;
use test::{Test, results::Results};

use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{Shell, generate};
use crossterm::{
    self, cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute, terminal,
};
use rand::seq::SliceRandom;
use ratatui::{Terminal, backend::CrosstermBackend};
use rust_embed::RustEmbed;
use std::{
    ffi::OsString,
    fs,
    io::{self, Read},
    num,
    path::PathBuf,
    str,
    sync::Arc,
    time::{Duration, Instant},
};

#[derive(RustEmbed)]
#[folder = "resources/runtime"]
struct Resources;

#[derive(Debug, Parser)]
#[command(about, version)]
struct Opt {
    /// Read test contents from the specified file, or "-" for stdin
    #[arg(value_name = "PATH")]
    contents: Option<PathBuf>,

    #[arg(short, long)]
    debug: bool,

    /// Specify word count
    #[arg(short, long, value_name = "N", default_value = "50")]
    words: num::NonZeroUsize,

    /// Use config file
    #[arg(short, long, value_name = "PATH")]
    config: Option<PathBuf>,

    /// Specify test language in file
    #[arg(long, value_name = "PATH")]
    language_file: Option<PathBuf>,

    /// Specify test language
    #[arg(short, long, value_name = "LANG")]
    language: Option<String>,

    /// List installed languages
    #[arg(long)]
    list_languages: bool,

    /// Disable backtracking to completed words
    #[arg(long)]
    no_backtrack: bool,

    /// Enable sudden death mode to restart on first error
    #[arg(long)]
    sudden_death: bool,

    /// Disable backspace
    #[arg(long)]
    no_backspace: bool,

    /// Display all but skip non-ASCII characters during typing
    #[arg(long)]
    ascii: bool,

    /// Ignore saved progress for the input file and start from the beginning
    #[arg(long)]
    restart: bool,

    /// Do not persist per-document progress for this invocation
    #[arg(long)]
    no_save: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Generate shell completions
    Completions {
        /// Shell to generate completions for
        shell: Shell,
    },
}

impl Opt {
    /// Generate test contents.
    ///
    /// Returns `Ok(Some(content))` on success. Returns `Ok(None)` only in
    /// language mode when no matching language file is found, and `Err` on
    /// I/O failure reading a file or stdin.
    fn gen_contents(&self) -> io::Result<Option<Arc<Content>>> {
        match &self.contents {
            Some(path) => {
                let label = source_label(path);
                if path.as_os_str() == "-" {
                    let mut buf = String::new();
                    std::io::stdin().lock().read_to_string(&mut buf)?;
                    // Word ranges are u32; reject oversized stdin up front.
                    if buf.len() > u32::MAX as usize {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidInput,
                            format!(
                                "stdin input is {} bytes; ttypo supports up to {} bytes (4 GiB)",
                                buf.len(),
                                u32::MAX,
                            ),
                        ));
                    }
                    Ok(Some(Arc::new(Content::from_text(buf, label))))
                } else {
                    Ok(Some(Arc::new(Content::from_file(path, label)?)))
                }
            }
            None => {
                let lang_name = self
                    .language
                    .clone()
                    .unwrap_or_else(|| self.config().default_language);

                let Some(bytes) = self
                    .language_file
                    .as_ref()
                    .map(fs::read)
                    .and_then(Result::ok)
                    .or_else(|| fs::read(self.language_dir().join(&lang_name)).ok())
                    .or_else(|| {
                        Resources::get(&format!("language/{}", &lang_name))
                            .map(|f| f.data.into_owned())
                    })
                else {
                    return Ok(None);
                };

                let mut rng = rand::rng();

                let mut language: Vec<&str> = str::from_utf8(&bytes)
                    .expect("Language file had non-utf8 encoding.")
                    .lines()
                    .collect();
                language.shuffle(&mut rng);

                let mut words: Vec<String> = language
                    .into_iter()
                    .cycle()
                    .take(self.words.get())
                    .map(ToOwned::to_owned)
                    .collect();
                words.shuffle(&mut rng);

                Ok(Some(Arc::new(Content::from_word_list(words, lang_name))))
            }
        }
    }

    /// Configuration
    fn config(&self) -> Config {
        fs::read(
            self.config
                .clone()
                .unwrap_or_else(|| self.config_dir().join("config.toml")),
        )
        .map(|bytes| {
            toml::from_str(str::from_utf8(&bytes).unwrap_or_default())
                .expect("Configuration was ill-formed.")
        })
        .unwrap_or_default()
    }

    /// Installed languages under config directory
    fn languages(&self) -> io::Result<impl Iterator<Item = OsString> + use<>> {
        let builtin = Resources::iter().filter_map(|name| {
            name.strip_prefix("language/")
                .map(ToOwned::to_owned)
                .map(OsString::from)
        });

        let configured = self
            .language_dir()
            .read_dir()
            .into_iter()
            .flatten()
            .map_while(Result::ok)
            .map(|e| e.file_name());

        Ok(builtin.chain(configured))
    }

    /// Config directory
    fn config_dir(&self) -> PathBuf {
        dirs::config_dir()
            .expect("Failed to find config directory.")
            .join("ttypo")
    }

    /// Data directory (persistent state: per-document typing progress, etc.).
    /// Linux: `~/.local/share/ttypo`.
    fn data_dir(&self) -> PathBuf {
        dirs::data_dir()
            .expect("Failed to find data directory.")
            .join("ttypo")
    }

    /// Language directory under config directory
    fn language_dir(&self) -> PathBuf {
        self.config_dir().join("language")
    }

    /// Installed languages sorted and deduplicated.
    fn languages_sorted(&self) -> Vec<String> {
        let mut langs: Vec<String> = self
            .languages()
            .ok()
            .into_iter()
            .flatten()
            .filter_map(|os| os.into_string().ok())
            .collect();
        langs.sort();
        langs.dedup();
        langs
    }

    /// Validate that the language used in language mode resolves to a file.
    /// `--language-file` bypasses language-name lookup, so it's always ok.
    fn validate_language(&self, config: &Config) -> Result<(), String> {
        if self.language_file.is_some() {
            return Ok(());
        }
        let lang = self
            .language
            .clone()
            .unwrap_or_else(|| config.default_language.clone());
        let found = self.language_dir().join(&lang).is_file()
            || Resources::get(&format!("language/{}", &lang)).is_some();
        if found { Ok(()) } else { Err(lang) }
    }
}

/// Context carried for the duration of a file-mode session so we can save
/// progress without recomputing the canonical path / content hash each time.
struct ResumeCtx {
    canonical_path: PathBuf,
    content_hash: String,
    total_words: usize,
    source_label: String,
    save_enabled: bool,
}

fn save_progress(store: &mut ProgressStore, ctx: &ResumeCtx, word_index: usize) {
    if !ctx.save_enabled {
        return;
    }
    store.upsert(
        &ctx.canonical_path,
        progress::Entry {
            content_hash: ctx.content_hash.clone(),
            word_index,
            total_words: ctx.total_words,
            updated_at: progress::now_unix(),
            source_label: ctx.source_label.clone(),
        },
    );
    // Best-effort save; do not interrupt typing on I/O error.
    let _ = store.save();
}

fn source_label(path: &std::path::Path) -> String {
    if path.as_os_str() == "-" {
        "stdin".to_string()
    } else {
        path.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string())
    }
}

fn clear_progress(store: &mut ProgressStore, ctx: &ResumeCtx) {
    if !ctx.save_enabled {
        return;
    }
    store.remove(&ctx.canonical_path);
    let _ = store.save();
}

/// Save every this-many newly-typed words so a crash or kill doesn't lose
/// more than a handful of words on a long document.
const PERIODIC_SAVE_INTERVAL: usize = 50;

fn teardown() -> io::Result<()> {
    terminal::disable_raw_mode()?;
    execute!(
        io::stdout(),
        cursor::RestorePosition,
        cursor::Show,
        terminal::LeaveAlternateScreen,
    )?;
    Ok(())
}

enum State {
    Test(Test),
    Results(Results),
}

impl State {
    fn render_into(
        &self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
        config: &Config,
        kb: &KeyboardState,
        kb_visible: bool,
    ) -> io::Result<()> {
        terminal.draw(|f: &mut ratatui::Frame| {
            // Hide the keyboard widget on the Results screen, since the static
            // "last key down" rendering would be misleading. Per-action key
            // glyphs are drawn inside the Results view itself.
            let show_kb = kb_visible && !matches!(self, State::Results(_));
            let (display, kb_rect) = split_with_keyboard(f.area(), show_kb);
            match self {
                State::Test(test) => {
                    f.render_widget(config.theme.apply_to(test), display);
                }
                State::Results(results) => {
                    f.render_widget(config.theme.apply_to(results), display);
                }
            }
            if let Some(r) = kb_rect {
                f.render_widget(config.theme.apply_to(KeyboardWidget::new(kb)), r);
            }
        })?;
        Ok(())
    }
}

fn main() -> io::Result<()> {
    let mut opt = Opt::parse();
    if opt.debug {
        dbg!(&opt);
    }

    let config = opt.config();
    if opt.debug {
        dbg!(&config);
    }

    if let Some(Command::Completions { shell }) = opt.command {
        generate(shell, &mut Opt::command(), "ttypo", &mut io::stdout());
        return Ok(());
    }

    if opt.list_languages {
        opt.languages()
            .unwrap()
            .for_each(|name| println!("{}", name.to_str().expect("Ill-formatted language name.")));

        return Ok(());
    }

    // Validate language up front (language mode only). Fail early before any
    // terminal takeover so error + help render cleanly.
    if opt.contents.is_none()
        && let Err(lang) = opt.validate_language(&config)
    {
        eprintln!("error: language \"{}\" not found.\n", lang);
        let _ = Opt::command().print_help();
        std::process::exit(1);
    }

    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;

    // File/stdin mode: read contents BEFORE entering the alt screen so stdin
    // still points at the real TTY/pipe.
    let mut file_contents: Option<Arc<Content>> = if let Some(path) = opt.contents.as_ref() {
        let content = match opt.gen_contents() {
            Ok(Some(c)) => c,
            Ok(None) => {
                eprintln!("error: couldn't get test contents.");
                std::process::exit(1);
            }
            Err(e) => {
                if path.as_os_str() == "-" {
                    eprintln!("error: failed to read from stdin: {}", e);
                } else {
                    eprintln!("error: cannot read '{}': {}", path.display(), e);
                }
                std::process::exit(1);
            }
        };
        if content.is_empty() {
            eprintln!("Error: the provided file or language contains no words to type.");
            eprintln!("If you specified a file, make sure it isn't empty.");
            std::process::exit(1);
        }
        Some(content)
    } else {
        None
    };

    terminal::enable_raw_mode()?;
    execute!(
        io::stdout(),
        cursor::Hide,
        cursor::SavePosition,
        terminal::EnterAlternateScreen,
    )?;
    terminal.clear()?;

    let mut progress_store = ProgressStore::load(opt.data_dir());

    // Persistent keyboard state lives across all phases (Title → Test →
    // Results) so the keyboard widget stays anchored at the bottom of the
    // screen and Ctrl+K can toggle it from any phase.
    let mut kb = KeyboardState::new();
    let mut kb_visible = true;

    // Outer "session" loop: re-entered when the user hits 'm' on the results
    // screen to return to the main menu. File mode never re-enters since 'm'
    // is disabled there.
    'outer: loop {
        let is_file_mode = opt.contents.is_some();
        if !is_file_mode {
            let t = title::Title::new(
                opt.language
                    .clone()
                    .unwrap_or_else(|| config.default_language.clone()),
                opt.words,
                opt.sudden_death,
                opt.no_backtrack,
                opt.no_backspace,
                opt.ascii,
                opt.languages_sorted(),
            );
            match title::run(&mut terminal, &config, t, &mut kb, &mut kb_visible)? {
                title::Outcome::Quit => break 'outer,
                title::Outcome::Start(t) => {
                    opt.language = Some(t.language);
                    opt.words = t.words;
                    opt.sudden_death = t.sudden_death;
                    opt.no_backtrack = t.no_backtrack;
                    opt.no_backspace = t.no_backspace;
                    opt.ascii = t.ascii;
                }
            }
        }

        let content: Arc<Content> = match file_contents.take() {
            Some(c) => c,
            None => match opt.gen_contents() {
                Ok(Some(c)) => c,
                Ok(None) => {
                    let _ = teardown();
                    eprintln!("Couldn't get test contents.");
                    std::process::exit(1);
                }
                Err(e) => {
                    let _ = teardown();
                    eprintln!("error: {}", e);
                    std::process::exit(1);
                }
            },
        };
        if content.is_empty() {
            let _ = teardown();
            eprintln!("Error: the provided file or language contains no words to type.");
            std::process::exit(1);
        }

        let source = content.source_label().to_string();

        let saved_content: Option<Arc<Content>> = is_file_mode.then(|| Arc::clone(&content));

        // Build resume context for real-file mode (excludes stdin) so we can
        // look up and save per-document progress. Hash the already-loaded
        // bytes so we don't re-read the file.
        //
        // Note: when Content::from_file took the control-char fallback path,
        // `content.as_bytes()` is the sanitized owned buffer rather than the
        // raw file bytes. That means progress saved before the refactor
        // (hashed against raw file bytes) will report "hash doesn't match"
        // exactly once on resume, after which the new sanitized hash takes
        // over. Acceptable: the worst case is a single false-positive change
        // warning per legacy file.
        let resume_ctx: Option<ResumeCtx> = match &opt.contents {
            Some(path) if path.as_os_str() != "-" => Some(ResumeCtx {
                canonical_path: progress::canonicalize(path),
                content_hash: progress::hash_bytes(content.as_bytes()),
                total_words: content.word_count(),
                source_label: source.clone(),
                save_enabled: !opt.no_save,
            }),
            _ => None,
        };

        // If the user previously saved progress for this file, prompt to resume.
        let mut initial_word_index: usize = 0;
        if let Some(ctx) = resume_ctx.as_ref()
            && ctx.save_enabled
            && !opt.restart
            && let Some(entry) = progress_store.lookup(&ctx.canonical_path).cloned()
            && entry.word_index > 0
        {
            let hash_matches = entry.content_hash == ctx.content_hash;
            let info = resume_prompt::ResumeInfo {
                source_label: ctx.source_label.clone(),
                word_index: entry.word_index,
                total_words: ctx.total_words,
                updated_at: entry.updated_at,
                hash_matches,
            };
            match resume_prompt::run(&mut terminal, &config, &info)? {
                resume_prompt::Outcome::Resume => {
                    initial_word_index = entry.word_index.min(ctx.total_words.saturating_sub(1));
                }
                resume_prompt::Outcome::Fresh => {}
                resume_prompt::Outcome::Discard => {
                    clear_progress(&mut progress_store, ctx);
                }
                resume_prompt::Outcome::Quit => break 'outer,
            }
        }

        let make_test = |content: Arc<Content>, source: String| {
            Test::new(
                content,
                !opt.no_backtrack,
                opt.sudden_death,
                !opt.no_backspace,
                opt.ascii,
                source,
            )
        };

        // Restart picks up the same backing Content via Arc::clone: no
        // re-read, no re-tokenize, no deep copy of the word list.
        let restart_content = || -> Option<Arc<Content>> {
            saved_content
                .as_ref()
                .map(Arc::clone)
                .or_else(|| opt.gen_contents().ok().flatten())
        };

        let mut paused_test: Option<(Test, bool)> = None;
        let mut is_original_test = true;
        let mut last_saved_word = initial_word_index;

        let mut initial_test = make_test(Arc::clone(&content), source.clone());
        if initial_word_index > 0 {
            initial_test.resume_at(initial_word_index);
        }
        let mut state = State::Test(initial_test);

        state.render_into(&mut terminal, &config, &kb, kb_visible)?;
        'session: loop {
            // Poll with a timeout that wakes for either the live status
            // refresh (timer/WPM during an active test) or any pending
            // keyboard-flash decay, whichever comes first.
            let test_running = matches!(&state, State::Test(t) if t.start_time.is_some());
            let mut timeout = if test_running {
                Duration::from_millis(200)
            } else {
                Duration::from_secs(3600)
            };
            if let Some(d) = kb.next_deadline() {
                timeout = timeout.min(d.saturating_duration_since(Instant::now()));
            }
            if !event::poll(timeout)? {
                kb.tick();
                if test_running || kb.has_active_flashes() {
                    state.render_into(&mut terminal, &config, &kb, kb_visible)?;
                }
                continue;
            }
            let event = event::read()?;

            // Flash any key press on the persistent keyboard before
            // dispatching to the active screen.
            if let Event::Key(ke) = &event
                && ke.kind == KeyEventKind::Press
            {
                kb.note_event(ke);
            }

            // Toggle the keyboard widget on Ctrl+K from any phase.
            if let Event::Key(KeyEvent {
                code: KeyCode::Char('k'),
                kind: KeyEventKind::Press,
                modifiers: KeyModifiers::CONTROL,
                ..
            }) = event
            {
                kb_visible = !kb_visible;
                state.render_into(&mut terminal, &config, &kb, kb_visible)?;
                continue;
            }

            // handle exit controls
            match event {
                Event::Key(KeyEvent {
                    code: KeyCode::Char('c'),
                    kind: KeyEventKind::Press,
                    modifiers: KeyModifiers::CONTROL,
                    ..
                }) => {
                    // Best-effort save on Ctrl-C so unpaused progress isn't lost.
                    if is_original_test
                        && let (State::Test(test), Some(ctx)) = (&state, resume_ctx.as_ref())
                    {
                        save_progress(&mut progress_store, ctx, test.current_word);
                    }
                    break 'outer;
                }
                Event::Key(KeyEvent {
                    code: KeyCode::Esc,
                    kind: KeyEventKind::Press,
                    modifiers: KeyModifiers::NONE,
                    ..
                }) => {
                    state = match state {
                        State::Test(test) => {
                            // Aborting before typing anything: skip the empty
                            // results screen. In language mode, return to the
                            // title; in file mode there's no title to return
                            // to, so quit.
                            if !test.has_events() {
                                if is_file_mode {
                                    break 'outer;
                                } else {
                                    break 'session;
                                }
                            }
                            if is_original_test && let Some(ctx) = resume_ctx.as_ref() {
                                save_progress(&mut progress_store, ctx, test.current_word);
                                last_saved_word = test.current_word;
                            }
                            let mut results = Results::from(&test);
                            results.is_repeat = is_file_mode;
                            // Only the original test is resumable. Pausing a
                            // practice run drops it, preserving any original
                            // that's already parked in `paused_test`.
                            if is_original_test {
                                paused_test = Some((test, true));
                            }
                            results.can_continue = paused_test.is_some();
                            State::Results(results)
                        }
                        State::Results(_) => break 'outer,
                    };
                }
                _ => {}
            }

            match state {
                State::Test(ref mut test) => {
                    if let Event::Key(key) = event {
                        if test.handle_key(key) {
                            kb.mark_wrong(&key);
                        }
                        if test.complete {
                            if is_original_test {
                                if let Some(ctx) = resume_ctx.as_ref() {
                                    clear_progress(&mut progress_store, ctx);
                                }
                                // Finishing the original discards any paused
                                // parent. Finishing a practice run leaves the
                                // paused original intact so 'c' still works.
                                paused_test = None;
                            }
                            let mut results = Results::from(&*test);
                            results.is_repeat = is_file_mode;
                            results.can_continue = paused_test.is_some();
                            state = State::Results(results);
                        } else if is_original_test
                            && test.current_word >= last_saved_word + PERIODIC_SAVE_INTERVAL
                            && let Some(ctx) = resume_ctx.as_ref()
                        {
                            save_progress(&mut progress_store, ctx, test.current_word);
                            last_saved_word = test.current_word;
                        }
                    }
                }
                State::Results(ref result) => {
                    if let Event::Key(KeyEvent {
                        code: KeyCode::Char(c),
                        kind: KeyEventKind::Press,
                        ..
                    }) = event
                    {
                        match c.to_ascii_lowercase() {
                            'r' => {
                                let Some(new_content) = restart_content() else {
                                    continue;
                                };
                                if new_content.is_empty() {
                                    continue;
                                }
                                state = State::Test(make_test(new_content, source.clone()));
                                is_original_test = true;
                                last_saved_word = 0;
                            }
                            'p' => {
                                if result.missed_words.is_empty() {
                                    continue;
                                }
                                let mut practice_words: Vec<String> = result
                                    .missed_words
                                    .iter()
                                    .flat_map(|(w, _)| std::iter::repeat_n(w.clone(), 5))
                                    .collect();
                                practice_words.shuffle(&mut rand::rng());
                                let practice_content = Arc::new(Content::from_word_list(
                                    practice_words,
                                    "practice".to_string(),
                                ));
                                state = State::Test(make_test(
                                    practice_content,
                                    "practice".to_string(),
                                ));
                                is_original_test = false;
                            }
                            'c' => {
                                if let Some((test, was_original)) = paused_test.take() {
                                    state = State::Test(test);
                                    is_original_test = was_original;
                                }
                            }
                            'q' => break 'outer,
                            'm' if !is_file_mode => break 'session,
                            _ => {}
                        }
                    }
                }
            }

            state.render_into(&mut terminal, &config, &kb, kb_visible)?;
        }
    }

    teardown()?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn make_opt(path: PathBuf, ascii: bool) -> Opt {
        Opt {
            contents: Some(path),
            debug: false,
            words: num::NonZeroUsize::new(50).unwrap(),
            config: None,
            language_file: None,
            language: None,
            list_languages: false,
            no_backtrack: false,
            sudden_death: false,
            no_backspace: false,
            ascii,
            restart: false,
            no_save: false,
            command: None,
        }
    }

    /// Collect words + lines from the Content produced by `gen_contents`.
    fn parts(opt: &Opt) -> (Vec<String>, Vec<test::DisplayLine>) {
        let content = opt.gen_contents().unwrap().unwrap();
        let words: Vec<String> = (0..content.word_count())
            .map(|i| content.word(i).to_string())
            .collect();
        (words, content.lines.clone())
    }

    #[test]
    fn gen_contents_empty_file_returns_empty_vec() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.txt");
        fs::File::create(&path).unwrap();

        let (contents, lines) = parts(&make_opt(path, false));
        assert!(contents.is_empty(), "empty file should produce empty vec");
        assert!(lines.is_empty());
    }

    #[test]
    fn gen_contents_splits_words() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("words.txt");
        let mut f = fs::File::create(&path).unwrap();
        writeln!(f, "hello world rust").unwrap();

        let (contents, lines) = parts(&make_opt(path, false));
        assert_eq!(contents, vec!["hello", "world", "rust"]);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].word_start, 0);
        assert_eq!(lines[0].word_count, 3);
    }

    #[test]
    fn gen_contents_preserves_unicode() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("unicode.txt");
        let mut f = fs::File::create(&path).unwrap();
        writeln!(f, "hello—world “quoted”").unwrap();

        let (contents, _) = parts(&make_opt(path, false));
        assert_eq!(
            contents,
            vec!["hello—world", "“quoted”"]
        );
    }

    #[test]
    fn gen_contents_multiline_tracks_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("multi.txt");
        let mut f = fs::File::create(&path).unwrap();
        write!(f, "first line\nsecond line\n\nfourth line").unwrap();

        let (contents, lines) = parts(&make_opt(path, false));
        assert_eq!(
            contents,
            vec!["first", "line", "second", "line", "fourth", "line"]
        );
        // 4 lines: line 1, line 2, empty line, line 4
        assert_eq!(lines.len(), 4);
        assert_eq!((lines[0].word_start, lines[0].word_count), (0, 2));
        assert_eq!((lines[1].word_start, lines[1].word_count), (2, 2));
        assert_eq!(lines[2].word_count, 0); // empty line preserved
        assert_eq!((lines[3].word_start, lines[3].word_count), (4, 2));
    }

    #[test]
    fn gen_contents_preserves_whitespace_only_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("spaces.txt");
        let mut f = fs::File::create(&path).unwrap();
        write!(f, "hello\n   \n  \t  \nworld").unwrap();

        let (contents, lines) = parts(&make_opt(path, false));
        assert_eq!(contents, vec!["hello", "world"]);
        // 4 lines total: "hello", whitespace-only, whitespace-only, "world"
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0].word_count, 1);
        assert_eq!(lines[1].word_count, 0); // whitespace-only preserved
        assert_eq!(lines[2].word_count, 0);
        assert_eq!(lines[3].word_count, 1);
    }

    #[test]
    fn gen_contents_keeps_all_unicode_tokens() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("alluni.txt");
        let mut f = fs::File::create(&path).unwrap();
        write!(f, "hello ——— world").unwrap();

        let (contents, _) = parts(&make_opt(path, false));
        assert_eq!(contents, vec!["hello", "———", "world"]);
    }

    #[test]
    fn gen_contents_preserves_punctuation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("punct.txt");
        let mut f = fs::File::create(&path).unwrap();
        write!(f, "it's a \"test\" (100%); done!").unwrap();

        let (contents, _) = parts(&make_opt(path, false));
        assert_eq!(contents, vec!["it's", "a", "\"test\"", "(100%);", "done!"]);
    }

    #[test]
    fn gen_contents_expands_tabs_in_indent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tabs.txt");
        let mut f = fs::File::create(&path).unwrap();
        write!(f, "hello\n\tindented\n\t\tdeep").unwrap();

        let (contents, lines) = parts(&make_opt(path, false));
        assert_eq!(contents, vec!["hello", "indented", "deep"]);
        assert_eq!(lines[0].indent, "");
        assert_eq!(lines[1].indent, "    "); // 1 tab = 4 spaces
        assert_eq!(lines[2].indent, "        "); // 2 tabs = 8 spaces
    }

    #[test]
    fn gen_contents_strips_control_chars_from_words() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ctrl.txt");
        let mut f = fs::File::create(&path).unwrap();
        write!(f, "hel\x07lo wor\x00ld").unwrap();

        let (contents, _) = parts(&make_opt(path, false));
        assert_eq!(contents, vec!["hello", "world"]);
    }
}
