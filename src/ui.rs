use crate::config::Theme;

use super::test::{results, Test, TestWord};

use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
    symbols::Marker,
    text::{Line, Span, Text},
    widgets::{Axis, Block, Borders, Chart, Dataset, GraphType, Paragraph, Widget, Wrap},
};
use results::Fraction;

// Convert CPS to WPM (clicks per second)
const WPM_PER_CPS: f64 = 12.0;

// Width of the moving average window for the WPM chart
const WPM_SMA_WIDTH: usize = 10;

pub trait ThemedWidget {
    fn render(self, area: Rect, buf: &mut Buffer, theme: &Theme);
}

pub struct Themed<'t, W: ?Sized> {
    theme: &'t Theme,
    widget: W,
}
impl<W: ThemedWidget> Widget for Themed<'_, W> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        self.widget.render(area, buf, self.theme)
    }
}
impl Theme {
    pub fn apply_to<W>(&self, widget: W) -> Themed<'_, W> {
        Themed {
            theme: self,
            widget,
        }
    }
}

impl ThemedWidget for &Test {
    fn render(self, area: Rect, buf: &mut Buffer, theme: &Theme) {
        buf.set_style(area, theme.default);

        // Center content on wide terminals
        let h_margin = if area.width > 90 {
            (area.width - 90) / 2
        } else {
            0
        };
        let padded = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(h_margin),
                Constraint::Min(1),
                Constraint::Length(h_margin),
            ])
            .split(area)[1];

        // Use flexible prompt height in file mode so more text is visible
        let prompt_constraint = if !self.lines.is_empty() {
            Constraint::Min(6)
        } else {
            Constraint::Length(6)
        };

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                prompt_constraint,
                Constraint::Length(1),
            ])
            .split(padded);

        // Stats line - centered
        let (done, total) = self.progress();
        let elapsed = self.elapsed_secs();
        let mins = (elapsed as u64) / 60;
        let secs = (elapsed as u64) % 60;
        let wpm = self.live_wpm();
        let sep = Span::styled(" \u{2502} ", theme.status_timer);

        let stats_line = Line::from(vec![
            Span::styled(format!("{:.0} wpm", wpm), theme.status_wpm),
            sep.clone(),
            Span::styled(format!("{:01}:{:02}", mins, secs), theme.status_timer),
            sep,
            Span::styled(format!("{}/{}", done, total), theme.status_progress),
        ]);
        let stats_width: usize = stats_line.spans.iter().map(|s| s.width()).sum();
        let stats_offset = chunks[0]
            .width
            .saturating_sub(stats_width as u16)
            / 2;
        buf.set_line(chunks[0].x + stats_offset, chunks[0].y, &stats_line, chunks[0].width);

        // Progress bar - full width of the prompt area
        let progress_frac = if total > 0 {
            done as f64 / total as f64
        } else {
            0.0
        };
        let bar_width = chunks[2].width as usize;
        let filled = (progress_frac * bar_width as f64).round() as usize;
        let empty = bar_width.saturating_sub(filled);
        let bar_line = Line::from(vec![
            Span::styled(
                "\u{2588}".repeat(filled),
                theme.status_progress_filled,
            ),
            Span::styled(
                "\u{2591}".repeat(empty),
                theme.status_progress_empty,
            ),
        ]);
        buf.set_line(chunks[2].x, chunks[2].y, &bar_line, chunks[2].width);

        let target_lines: Vec<Line> = {
            let words = words_to_spans(&self.words, self.current_word, theme, self.ascii);

            if !self.lines.is_empty() {
                // File mode: preserve original line structure with indentation
                let mut display: Vec<Line> = Vec::new();
                for dl in &self.lines {
                    if dl.word_count == 0 {
                        // Empty line
                        display.push(Line::from(""));
                    } else {
                        let mut line_spans: Vec<Span> = Vec::new();
                        if !dl.indent.is_empty() {
                            line_spans.push(Span::raw(dl.indent.clone()));
                        }
                        let end = dl.word_start + dl.word_count;
                        for word in &words[dl.word_start..end] {
                            line_spans.extend(word.iter().cloned());
                        }
                        display.push(Line::from(line_spans));
                    }
                }

                // Scroll to keep the current line visible
                let available = chunks[1].height.saturating_sub(2) as usize;
                let current_line_idx = self
                    .lines
                    .iter()
                    .position(|dl| {
                        dl.word_count > 0
                            && self.current_word >= dl.word_start
                            && self.current_word < dl.word_start + dl.word_count
                    })
                    .unwrap_or(0);
                let scroll = current_line_idx.saturating_sub(available / 2);
                display.into_iter().skip(scroll).take(available).collect()
            } else {
                // Language mode: wrap words at terminal width
                let mut lines: Vec<Line> = Vec::new();
                let mut current_line: Vec<Span> = Vec::new();
                let mut current_width = 0;
                for word in words {
                    let word_width: usize = word.iter().map(|s| s.width()).sum();

                    if current_width + word_width > chunks[1].width as usize - 2 {
                        current_line.push(Span::raw("\n"));
                        lines.push(Line::from(current_line.clone()));
                        current_line.clear();
                        current_width = 0;
                    }

                    current_line.extend(word);
                    current_width += word_width;
                }
                lines.push(Line::from(current_line));
                lines
            }
        };
        let target = Paragraph::new(target_lines)
            .wrap(Wrap { trim: false })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(theme.border_type)
                    .border_style(theme.prompt_border),
            );
        target.render(chunks[1], buf);
    }
}

fn words_to_spans<'a>(
    words: &'a [TestWord],
    current_word: usize,
    theme: &'a Theme,
    ascii: bool,
) -> Vec<Vec<Span<'a>>> {
    let mut spans = Vec::new();

    for word in &words[..current_word] {
        let parts = split_typed_word(word, ascii);
        spans.push(word_parts_to_spans(parts, theme));
    }

    let parts_current = split_current_word(&words[current_word], ascii);
    spans.push(word_parts_to_spans(parts_current, theme));

    for word in &words[current_word + 1..] {
        let parts = vec![(word.text.clone(), Status::Untyped)];
        spans.push(word_parts_to_spans(parts, theme));
    }
    spans
}

#[derive(PartialEq, Clone, Copy, Debug)]
enum Status {
    Correct,
    Incorrect,
    CurrentUntyped,
    CurrentCorrect,
    CurrentIncorrect,
    Cursor,
    Untyped,
    Overtyped,
    Skipped,
}

fn split_current_word(word: &TestWord, ascii: bool) -> Vec<(String, Status)> {
    use super::test::is_typeable;

    let mut parts = Vec::new();
    let mut cur_string = String::new();
    let mut cur_status = Status::Untyped;

    let mut progress = word.progress.chars();
    for tc in word.text.chars() {
        // In ascii mode, non-typeable chars are displayed but skipped over
        if ascii && !is_typeable(tc) {
            let status = Status::Skipped;
            if status == cur_status {
                cur_string.push(tc);
            } else {
                if !cur_string.is_empty() {
                    parts.push((cur_string, cur_status));
                    cur_string = String::new();
                }
                cur_string.push(tc);
                cur_status = status;
            }
            continue;
        }

        let p = progress.next();
        let status = match p {
            None => Status::CurrentUntyped,
            Some(c) => match c {
                c if c == tc => Status::CurrentCorrect,
                _ => Status::CurrentIncorrect,
            },
        };

        if status == cur_status {
            cur_string.push(tc);
        } else {
            if !cur_string.is_empty() {
                parts.push((cur_string, cur_status));
                cur_string = String::new();
            }
            cur_string.push(tc);
            cur_status = status;

            // first currentuntyped is cursor
            if status == Status::CurrentUntyped {
                parts.push((cur_string, Status::Cursor));
                cur_string = String::new();
            }
        }
    }
    if !cur_string.is_empty() {
        parts.push((cur_string, cur_status));
    }
    let overtyped = progress.collect::<String>();
    if !overtyped.is_empty() {
        parts.push((overtyped, Status::Overtyped));
    }
    parts
}

fn split_typed_word(word: &TestWord, ascii: bool) -> Vec<(String, Status)> {
    use super::test::is_typeable;

    let mut parts = Vec::new();
    let mut cur_string = String::new();
    let mut cur_status = Status::Untyped;

    let mut progress = word.progress.chars();
    for tc in word.text.chars() {
        if ascii && !is_typeable(tc) {
            let status = Status::Skipped;
            if status == cur_status {
                cur_string.push(tc);
            } else {
                if !cur_string.is_empty() {
                    parts.push((cur_string, cur_status));
                    cur_string = String::new();
                }
                cur_string.push(tc);
                cur_status = status;
            }
            continue;
        }

        let p = progress.next();
        let status = match p {
            None => Status::Untyped,
            Some(c) => match c {
                c if c == tc => Status::Correct,
                _ => Status::Incorrect,
            },
        };

        if status == cur_status {
            cur_string.push(tc);
        } else {
            if !cur_string.is_empty() {
                parts.push((cur_string, cur_status));
                cur_string = String::new();
            }
            cur_string.push(tc);
            cur_status = status;
        }
    }
    if !cur_string.is_empty() {
        parts.push((cur_string, cur_status));
    }

    let overtyped = progress.collect::<String>();
    if !overtyped.is_empty() {
        parts.push((overtyped, Status::Overtyped));
    }
    parts
}

fn word_parts_to_spans(parts: Vec<(String, Status)>, theme: &Theme) -> Vec<Span<'_>> {
    let mut spans = Vec::new();
    for (text, status) in parts {
        let style = match status {
            Status::Correct => theme.prompt_correct,
            Status::Incorrect => theme.prompt_incorrect,
            Status::Untyped => theme.prompt_untyped,
            Status::CurrentUntyped => theme.prompt_current_untyped,
            Status::CurrentCorrect => theme.prompt_current_correct,
            Status::CurrentIncorrect => theme.prompt_current_incorrect,
            Status::Cursor => theme.prompt_current_untyped.patch(theme.prompt_cursor),
            Status::Overtyped => theme.prompt_incorrect,
            Status::Skipped => theme.prompt_skipped,
        };

        spans.push(Span::styled(text, style));
    }
    spans.push(Span::styled(" ", theme.prompt_untyped));
    spans
}

impl ThemedWidget for &results::Results {
    fn render(self, area: Rect, buf: &mut Buffer, theme: &Theme) {
        buf.set_style(area, theme.default);

        // Chunks
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(area);
        let res_chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1) // Graph looks tremendously better with just a little margin
            .constraints([Constraint::Ratio(1, 3), Constraint::Ratio(2, 3)])
            .split(chunks[0]);
        let info_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Ratio(1, 3),
                Constraint::Ratio(1, 3),
                Constraint::Ratio(1, 3),
            ])
            .split(res_chunks[0]);

        let msg = match (self.is_repeat, self.missed_words.is_empty()) {
            (true, true) => "Press 'q' to quit or 'r' to repeat the test",
            (true, false) => {
                "Press 'q' to quit, 'r' to repeat the test or 'p' to practice missed words"
            }
            (false, true) => "Press 'q' to quit or 'r' for another test",
            (false, false) => {
                "Press 'q' to quit, 'r' for another test or 'p' to practice missed words"
            }
        };

        let exit = Line::from(Span::styled(msg, theme.results_restart_prompt));
        let x_offset = chunks[1]
            .width
            .saturating_sub(msg.len() as u16)
            / 2;
        buf.set_line(chunks[1].x + x_offset, chunks[1].y, &exit, chunks[1].width);

        // Sections
        let mut overview_text = Text::styled("", theme.results_overview);
        overview_text.extend([
            Line::from(format!(
                "Adjusted WPM: {:.1}",
                self.timing.overall_cps * WPM_PER_CPS * f64::from(self.accuracy.overall)
            )),
            Line::from(format!(
                "Accuracy: {:.1}%",
                f64::from(self.accuracy.overall) * 100f64
            )),
            Line::from(format!(
                "Raw WPM: {:.1}",
                self.timing.overall_cps * WPM_PER_CPS
            )),
            Line::from(format!("Correct Keypresses: {}", self.accuracy.overall)),
        ]);
        let overview = Paragraph::new(overview_text).block(
            Block::default()
                .title(Span::styled("Overview", theme.title))
                .borders(Borders::ALL)
                .border_type(theme.border_type)
                .border_style(theme.results_overview_border),
        );
        overview.render(info_chunks[0], buf);

        let mut worst_keys: Vec<(&KeyEvent, &Fraction)> = self
            .accuracy
            .per_key
            .iter()
            .filter(|(key, _)| matches!(key.code, KeyCode::Char(_)))
            .collect();
        worst_keys.sort_unstable_by_key(|x| x.1);

        let mut worst_text = Text::styled("", theme.results_worst_keys);
        worst_text.extend(
            worst_keys
                .iter()
                .filter_map(|(key, acc)| {
                    if let KeyCode::Char(character) = key.code {
                        let key_accuracy = f64::from(**acc) * 100.0;
                        if key_accuracy != 100.0 {
                            Some(format!("- {} at {:.1}% accuracy", character, key_accuracy))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
                .take(5)
                .map(Line::from),
        );
        let worst = Paragraph::new(worst_text).block(
            Block::default()
                .title(Span::styled("Worst Keys", theme.title))
                .borders(Borders::ALL)
                .border_type(theme.border_type)
                .border_style(theme.results_worst_keys_border),
        );
        worst.render(info_chunks[1], buf);

        let mut missed_text = Text::styled("", theme.results_missed_words);
        if self.missed_words.is_empty() {
            missed_text.extend([Line::from("None!")]);
        } else {
            missed_text.extend(
                self.missed_words
                    .iter()
                    .take(info_chunks[2].height.saturating_sub(2) as usize)
                    .map(|w| Line::from(format!("- {}", w))),
            );
        }
        let missed = Paragraph::new(missed_text).block(
            Block::default()
                .title(Span::styled("Missed Words", theme.title))
                .borders(Borders::ALL)
                .border_type(theme.border_type)
                .border_style(theme.results_missed_words_border),
        );
        missed.render(info_chunks[2], buf);

        // Scale the smoothing window so long tests produce a clean line
        let chart_width = res_chunks[1].width as usize;
        let num_events = self.timing.per_event.len();
        let sma_width = if chart_width > 0 && num_events > chart_width * 2 {
            num_events / chart_width
        } else {
            WPM_SMA_WIDTH
        }
        .max(WPM_SMA_WIDTH);

        let wpm_sma_full: Vec<(f64, f64)> = self
            .timing
            .per_event
            .windows(sma_width)
            .enumerate()
            .map(|(i, window)| {
                (
                    (i + sma_width) as f64,
                    window.len() as f64 / window.iter().copied().sum::<f64>() * WPM_PER_CPS,
                )
            })
            .collect();

        // Downsample to at most chart_width points
        let step = if chart_width > 0 {
            (wpm_sma_full.len() / chart_width).max(1)
        } else {
            1
        };
        let wpm_sma: Vec<(f64, f64)> = wpm_sma_full
            .iter()
            .step_by(step)
            .copied()
            .collect();

        // Plot a point on the SMA curve for each missed word
        let missed = &self.timing.missed_word_event_indices;
        let mistake_points: Vec<(f64, f64)> = wpm_sma_full
            .iter()
            .filter(|(x, _)| {
                let idx = (*x as usize).saturating_sub(sma_width);
                missed.contains(&idx)
            })
            .copied()
            .collect();

        // Render the chart if possible
        if !wpm_sma.is_empty() {
            let wpm_sma_min = wpm_sma
                .iter()
                .map(|(_, x)| x)
                .fold(f64::INFINITY, |a, &b| a.min(b));
            let wpm_sma_max = wpm_sma
                .iter()
                .map(|(_, x)| x)
                .fold(f64::NEG_INFINITY, |a, &b| a.max(b));

            let mut wpm_datasets = vec![Dataset::default()
                .name("WPM")
                .marker(Marker::Braille)
                .graph_type(GraphType::Line)
                .style(theme.results_chart)
                .data(&wpm_sma)];

            if !mistake_points.is_empty() {
                wpm_datasets.push(
                    Dataset::default()
                        .name("Mistakes")
                        .marker(Marker::Braille)
                        .graph_type(GraphType::Scatter)
                        .style(theme.results_chart_mistakes)
                        .data(&mistake_points),
                );
            }

            let y_label_min = wpm_sma_min as u16;
            let y_label_max = (wpm_sma_max as u16).max(y_label_min + 6);

            let wpm_chart = Chart::new(wpm_datasets)
                .block(Block::default().title(vec![Span::styled("Chart", theme.title)]))
                .x_axis(
                    Axis::default()
                        .title(Span::styled("Keypresses", theme.results_chart_x))
                        .bounds([0.0, self.timing.per_event.len() as f64]),
                )
                .y_axis(
                    Axis::default()
                        .title(Span::styled(
                            format!("WPM ({}-keypress rolling average)", sma_width),
                            theme.results_chart_y,
                        ))
                        .bounds([wpm_sma_min, wpm_sma_max])
                        .labels(
                            (y_label_min..y_label_max)
                                .step_by(5)
                                .map(|n| Span::raw(format!("{}", n)))
                                .collect(),
                        ),
                );
            wpm_chart.render(res_chunks[1], buf);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod split_words {
        use super::Status::*;
        use super::*;

        struct TestCase {
            word: &'static str,
            progress: &'static str,
            expected: Vec<(&'static str, Status)>,
        }

        fn setup(test_case: TestCase) -> (TestWord, Vec<(String, Status)>) {
            let mut word = TestWord::from(test_case.word);
            word.progress = test_case.progress.to_string();

            let expected = test_case
                .expected
                .iter()
                .map(|(s, v)| (s.to_string(), *v))
                .collect::<Vec<_>>();

            (word, expected)
        }

        #[test]
        fn typed_words_split() {
            let cases = vec![
                TestCase {
                    word: "monkeytype",
                    progress: "monkeytype",
                    expected: vec![("monkeytype", Correct)],
                },
                TestCase {
                    word: "monkeytype",
                    progress: "monkeXtype",
                    expected: vec![("monke", Correct), ("y", Incorrect), ("type", Correct)],
                },
                TestCase {
                    word: "monkeytype",
                    progress: "monkeas",
                    expected: vec![("monke", Correct), ("yt", Incorrect), ("ype", Untyped)],
                },
            ];

            for case in cases {
                let (word, expected) = setup(case);
                let got = split_typed_word(&word, false);
                assert_eq!(got, expected);
            }
        }

        #[test]
        fn current_word_split() {
            let cases = vec![
                TestCase {
                    word: "monkeytype",
                    progress: "monkeytype",
                    expected: vec![("monkeytype", CurrentCorrect)],
                },
                TestCase {
                    word: "monkeytype",
                    progress: "monke",
                    expected: vec![
                        ("monke", CurrentCorrect),
                        ("y", Cursor),
                        ("type", CurrentUntyped),
                    ],
                },
                TestCase {
                    word: "monkeytype",
                    progress: "monkeXt",
                    expected: vec![
                        ("monke", CurrentCorrect),
                        ("y", CurrentIncorrect),
                        ("t", CurrentCorrect),
                        ("y", Cursor),
                        ("pe", CurrentUntyped),
                    ],
                },
            ];

            for case in cases {
                let (word, expected) = setup(case);
                let got = split_current_word(&word, false);
                assert_eq!(got, expected);
            }
        }

        #[test]
        fn typed_word_ascii_skips_unicode() {
            // Word "café" typed as "caf" - the é is shown as Skipped (yellow)
            let mut word = TestWord::from("caf\u{00e9}");
            word.progress = "caf".to_string();

            let got = split_typed_word(&word, true);
            let expected = vec![
                ("caf".to_string(), Correct),
                ("\u{00e9}".to_string(), Skipped),
            ];
            assert_eq!(got, expected);
        }

        #[test]
        fn current_word_ascii_skips_unicode() {
            // Word "café", user has typed "ca", cursor should be on 'f'
            let mut word = TestWord::from("caf\u{00e9}");
            word.progress = "ca".to_string();

            let got = split_current_word(&word, true);
            let expected = vec![
                ("ca".to_string(), CurrentCorrect),
                ("f".to_string(), Cursor),
                ("\u{00e9}".to_string(), Skipped),
            ];
            assert_eq!(got, expected);
        }
    }
}
