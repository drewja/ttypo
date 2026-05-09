use crate::config::Theme;
use crate::content::Content;

use super::test::{Test, TestWord, results};

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

        let prompt_constraint = Constraint::Min(6);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                prompt_constraint,
                Constraint::Length(1),
            ])
            .split(area);

        // Stats line - centered
        let (done, total) = self.progress();
        let elapsed = self.elapsed_secs();
        let mins = (elapsed as u64) / 60;
        let secs = (elapsed as u64) % 60;
        let wpm = self.live_wpm();
        let sep = Span::styled(" │ ", theme.status_timer);

        let stats_line = Line::from(vec![
            Span::styled(format!("{:>3.0} wpm", wpm), theme.status_wpm),
            sep.clone(),
            Span::styled(format!("{:01}:{:02}", mins, secs), theme.status_timer),
            sep,
            Span::styled(format!("{}/{}", done, total), theme.status_progress),
        ]);
        let stats_width: usize = stats_line.spans.iter().map(|s| s.width()).sum();
        let stats_offset = chunks[0].width.saturating_sub(stats_width as u16) / 2;
        buf.set_line(
            chunks[0].x + stats_offset,
            chunks[0].y,
            &stats_line,
            chunks[0].width,
        );

        // Right-aligned hint for toggling the keyboard widget. Suppressed if
        // it would collide with the centered stats line.
        let kb_hint = Line::from(vec![
            Span::styled("Ctrl+k", theme.status_wpm),
            Span::styled(" keyboard", theme.status_timer),
        ]);
        let hint_width: usize = kb_hint.spans.iter().map(|s| s.width()).sum();
        let stats_right = stats_offset as usize + stats_width;
        if chunks[0].width as usize >= stats_right + hint_width + 2 {
            let hint_x = chunks[0].x + chunks[0].width - hint_width as u16;
            buf.set_line(hint_x, chunks[0].y, &kb_hint, hint_width as u16);
        }

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
            Span::styled("█".repeat(filled), theme.status_progress_filled),
            Span::styled("░".repeat(empty), theme.status_progress_empty),
        ]);
        buf.set_line(chunks[2].x, chunks[2].y, &bar_line, chunks[2].width);

        let lines = self.lines();
        let is_file_mode = !lines.is_empty();
        let target_lines: Vec<Line> = if is_file_mode {
            // File mode: scroll first, then build spans only for visible lines.
            let available = chunks[1].height.saturating_sub(2) as usize;
            let current_line_idx = lines
                .iter()
                .position(|dl| {
                    dl.word_count > 0
                        && self.current_word >= dl.word_start
                        && self.current_word < dl.word_start + dl.word_count
                })
                .unwrap_or(0);
            let scroll = current_line_idx.saturating_sub(available / 2);

            lines
                .iter()
                .skip(scroll)
                .take(available)
                .map(|dl| {
                    if dl.word_count == 0 {
                        return Line::from("");
                    }
                    let mut spans: Vec<Span> = Vec::new();
                    if !dl.indent.is_empty() {
                        spans.push(Span::raw(dl.indent.as_str()));
                    }
                    for i in dl.word_start..dl.word_start + dl.word_count {
                        append_word_spans(
                            &mut spans,
                            &self.content,
                            &self.words[i],
                            i,
                            self.current_word,
                            theme,
                            self.ascii,
                        );
                    }
                    Line::from(spans)
                })
                .collect()
        } else {
            // Language mode: one Line; Paragraph::wrap handles line breaks.
            let mut spans: Vec<Span> = Vec::new();
            for (i, word) in self.words.iter().enumerate() {
                append_word_spans(
                    &mut spans,
                    &self.content,
                    word,
                    i,
                    self.current_word,
                    theme,
                    self.ascii,
                );
            }
            vec![Line::from(spans)]
        };
        let scroll_y: u16 = if is_file_mode {
            0
        } else {
            // Width inside borders + horizontal padding (1 each side).
            let inner_width = chunks[1].width.saturating_sub(4);
            let inner_height = chunks[1].height.saturating_sub(2);
            let current_row =
                current_wrap_row(&self.words, &self.content, self.current_word, inner_width);
            current_row.saturating_sub(inner_height / 2)
        };

        let target = Paragraph::new(target_lines)
            .wrap(Wrap { trim: false })
            .scroll((scroll_y, 0))
            .block(
                Block::default()
                    .title(Span::styled(self.source.clone(), theme.title))
                    .borders(Borders::ALL)
                    .border_type(theme.border_type)
                    .border_style(theme.prompt_border)
                    .padding(ratatui::widgets::Padding::horizontal(1)),
            );
        target.render(chunks[1], buf);
    }
}

fn truncate_with_ellipsis(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    if max == 0 {
        return String::new();
    }
    let mut out: String = s.chars().take(max - 1).collect();
    out.push('…');
    out
}

// Simulate ratatui's word-wrap to find which wrapped row holds `current_word`.
// Words separated by single spaces, greedy fit at `width` columns.
fn current_wrap_row(words: &[TestWord], content: &Content, current_word: usize, width: u16) -> u16 {
    let width = width.max(1) as usize;
    let mut row: usize = 0;
    let mut col: usize = 0;
    for (i, word) in words.iter().enumerate() {
        let text = &content.as_str()[word.range.start as usize..word.range.end as usize];
        let w_len = text.chars().count();
        if col == 0 {
            col = w_len;
        } else if col + 1 + w_len > width {
            row += 1;
            col = w_len;
        } else {
            col += 1 + w_len;
        }
        if i == current_word {
            return row.min(u16::MAX as usize) as u16;
        }
    }
    row.min(u16::MAX as usize) as u16
}

// Append `word`'s spans (styled according to its state relative to
// `current_word`) plus a trailing separator space onto `out`. Past/current
// words go through `split_word` (which allocates small pieces per status
// chunk); untyped words ahead of the cursor take a fast path that borrows
// the text slice directly out of the Content buffer with no allocation.
fn append_word_spans<'a>(
    out: &mut Vec<Span<'a>>,
    content: &'a Content,
    word: &'a TestWord,
    index: usize,
    current_word: usize,
    theme: &'a Theme,
    ascii: bool,
) {
    let text = &content.as_str()[word.range.start as usize..word.range.end as usize];
    if index < current_word {
        let parts = split_typed_word(word, text, ascii);
        extend_spans_from_parts(out, parts, theme);
    } else if index == current_word {
        let parts = split_current_word(word, text, ascii);
        extend_spans_from_parts(out, parts, theme);
    } else {
        // Fast path for untyped words: zero-copy borrow of text.
        out.push(Span::styled(text, theme.prompt_untyped));
    }
    out.push(Span::styled(" ", theme.prompt_untyped));
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

fn split_current_word(word: &TestWord, text: &str, ascii: bool) -> Vec<(String, Status)> {
    split_word(
        word,
        text,
        ascii,
        Status::CurrentUntyped,
        Status::CurrentCorrect,
        Status::CurrentIncorrect,
        true,
    )
}

fn split_typed_word(word: &TestWord, text: &str, ascii: bool) -> Vec<(String, Status)> {
    split_word(
        word,
        text,
        ascii,
        Status::Untyped,
        Status::Correct,
        Status::Incorrect,
        false,
    )
}

fn split_word(
    word: &TestWord,
    text: &str,
    ascii: bool,
    untyped: Status,
    correct: Status,
    incorrect: Status,
    emit_cursor: bool,
) -> Vec<(String, Status)> {
    use super::test::is_typeable;

    let mut parts: Vec<(String, Status)> = Vec::new();
    let mut cur_string = String::new();
    let mut cur_status = Status::Untyped;

    let flush = |parts: &mut Vec<(String, Status)>, cur_string: &mut String, cur_status| {
        if !cur_string.is_empty() {
            parts.push((std::mem::take(cur_string), cur_status));
        }
    };

    let mut progress = word.progress.chars();
    for tc in text.chars() {
        let status = if ascii && !is_typeable(tc) {
            Status::Skipped
        } else {
            match progress.next() {
                None => untyped,
                Some(c) if c == tc => correct,
                Some(_) => incorrect,
            }
        };

        if status == cur_status {
            cur_string.push(tc);
            continue;
        }

        flush(&mut parts, &mut cur_string, cur_status);
        cur_string.push(tc);
        cur_status = status;

        if emit_cursor && status == untyped {
            parts.push((std::mem::take(&mut cur_string), Status::Cursor));
        }
    }
    flush(&mut parts, &mut cur_string, cur_status);

    let overtyped = progress.collect::<String>();
    if !overtyped.is_empty() {
        parts.push((overtyped, Status::Overtyped));
    }
    parts
}

fn extend_spans_from_parts<'a>(
    out: &mut Vec<Span<'a>>,
    parts: Vec<(String, Status)>,
    theme: &Theme,
) {
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
        out.push(Span::styled(text, style));
    }
}

struct Control {
    label: char,
    desc: &'static str,
    right_style: bool,
}

const KEY_W: u16 = 7;
const KEY_GAP: u16 = 3;
const ART_H: u16 = 4;

fn key_art(label: char, right_style: bool) -> [String; 4] {
    if right_style {
        [
            "┏────┐┓".to_string(),
            format!("│  {} ││", label),
            "├────\\│".to_string(),
            "┗─────┛".to_string(),
        ]
    } else {
        [
            "┏┌────┓".to_string(),
            format!("││ {}  │", label),
            "│/────┤".to_string(),
            "┗─────┛".to_string(),
        ]
    }
}

fn control_item_w(c: &Control) -> u16 {
    KEY_W + 1 + c.desc.chars().count() as u16
}

// Greedy pack `controls` into rows that fit within `width`.
// Returns the number of controls per row.
fn pack_control_rows(controls: &[Control], width: u16) -> Vec<usize> {
    let mut rows: Vec<usize> = Vec::new();
    let mut row_count: usize = 0;
    let mut row_w: u16 = 0;
    for c in controls {
        let item_w = control_item_w(c);
        if row_count == 0 {
            row_w = item_w;
            row_count = 1;
        } else if row_w + KEY_GAP + item_w <= width {
            row_w += KEY_GAP + item_w;
            row_count += 1;
        } else {
            rows.push(row_count);
            row_count = 1;
            row_w = item_w;
        }
    }
    if row_count > 0 {
        rows.push(row_count);
    }
    rows
}

// Vertical rows of key art needed to render `controls` at `width`.
// Returns 0 if any tile would overflow (caller should fall back to text)
fn control_art_rows(controls: &[Control], width: u16) -> u16 {
    if controls.is_empty() {
        return 0;
    }
    let widest = controls.iter().map(control_item_w).max().unwrap_or(0);
    if widest > width {
        return 0;
    }
    pack_control_rows(controls, width).len() as u16
}

// Draw rows of key glyphs with descriptions, centered in `area`.
// Returns false if the area is too small so the caller can fall back to text.
fn render_controls(controls: &[Control], area: Rect, buf: &mut Buffer, theme: &Theme) -> bool {
    if controls.is_empty() {
        return true;
    }
    let widest = controls.iter().map(control_item_w).max().unwrap_or(0);
    if widest > area.width || area.height < ART_H {
        return false;
    }
    let row_breaks = pack_control_rows(controls, area.width);
    let n_rows = row_breaks.len() as u16;
    let needed_h = ART_H * n_rows + n_rows.saturating_sub(1);
    if needed_h > area.height {
        return false;
    }
    let outline = theme.prompt_untyped;
    let label_style = theme.title;
    let desc_style = theme.results_restart_prompt;

    let mut idx = 0;
    let mut y_off: u16 = 0;
    for &row_n in &row_breaks {
        let row_slice = &controls[idx..idx + row_n];
        let row_total_w: u16 = row_slice
            .iter()
            .enumerate()
            .map(|(i, c)| control_item_w(c) + if i + 1 < row_n { KEY_GAP } else { 0 })
            .sum();
        let mut x = area.x + area.width.saturating_sub(row_total_w) / 2;
        let row_y = area.y + y_off;
        for c in row_slice {
            let art = key_art(c.label, c.right_style);
            for (row_off, line_str) in art.iter().enumerate() {
                let y = row_y + row_off as u16;
                for (col, ch) in line_str.chars().enumerate() {
                    let style = if ch == c.label { label_style } else { outline };
                    buf.set_string(x + col as u16, y, ch.to_string(), style);
                }
            }
            buf.set_string(x + KEY_W + 1, row_y + 1, c.desc, desc_style);
            x += control_item_w(c) + KEY_GAP;
        }
        idx += row_n;
        y_off += ART_H + 1;
    }
    true
}

impl ThemedWidget for &results::Results {
    fn render(self, area: Rect, buf: &mut Buffer, theme: &Theme) {
        buf.set_style(area, theme.default);

        let mut controls: Vec<Control> = vec![Control {
            label: 'Q',
            desc: "quit",
            right_style: false,
        }];
        if self.can_continue {
            controls.push(Control {
                label: 'C',
                desc: "continue",
                right_style: false,
            });
        }
        if self.is_repeat {
            controls.push(Control {
                label: 'R',
                desc: "repeat",
                right_style: false,
            });
        } else {
            controls.push(Control {
                label: 'R',
                desc: "new test",
                right_style: false,
            });
            controls.push(Control {
                label: 'M',
                desc: "main menu",
                right_style: true,
            });
        }
        if !self.missed_words.is_empty() {
            controls.push(Control {
                label: 'P',
                desc: "practice missed words",
                right_style: true,
            });
        }

        // Size the bottom band to fit however many rows of key art the width
        // demands. The chart container's .margin(1) above already provides
        // the breathing row between chart and key hints.
        let art_rows = control_art_rows(&controls, area.width);
        let bottom_h: u16 = if art_rows > 0 {
            ART_H * art_rows + art_rows.saturating_sub(1)
        } else {
            1
        };

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(bottom_h)])
            .split(area);
        let res_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(6), Constraint::Min(0)])
            .split(chunks[0]);
        let info_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Ratio(1, 3),
                Constraint::Ratio(1, 3),
                Constraint::Ratio(1, 3),
            ])
            .split(res_chunks[0]);

        if !render_controls(&controls, chunks[1], buf, theme) {
            // Narrow terminal: fall back to a single-line text legend.
            let parts: Vec<String> = controls
                .iter()
                .map(|c| format!("'{}' {}", c.label.to_ascii_lowercase(), c.desc))
                .collect();
            let msg = parts.join(" | ");
            let msg_len = msg.chars().count() as u16;
            let exit = Line::from(Span::styled(msg, theme.results_restart_prompt));
            let bottom_y = chunks[1].y + chunks[1].height - 1;
            let x_offset = chunks[1].width.saturating_sub(msg_len) / 2;
            buf.set_line(chunks[1].x + x_offset, bottom_y, &exit, chunks[1].width);
        }

        // Sections
        let overview_text = Text::from(vec![
            Line::from(format!(
                "{:.1} WPM Adjusted",
                self.timing.overall_cps * WPM_PER_CPS * f64::from(self.accuracy.overall)
            )),
            Line::from(format!(
                "{:.1}% Accuracy",
                f64::from(self.accuracy.overall) * 100f64
            )),
            Line::from(format!(
                "{:.1} WPM Raw",
                self.timing.overall_cps * WPM_PER_CPS
            )),
            Line::from(format!("{} Hits", self.accuracy.overall)),
        ]);
        let overview = Paragraph::new(overview_text).block(
            Block::default()
                .title(Span::styled("Overview", theme.title))
                .borders(Borders::ALL)
                .border_type(theme.border_type)
                .border_style(theme.results_overview_border)
                .padding(ratatui::widgets::Padding::horizontal(1)),
        );
        overview.render(info_chunks[0], buf);

        let mut worst_keys: Vec<(char, &Fraction)> = self
            .accuracy
            .per_key
            .iter()
            .filter(|(c, acc)| **c != ' ' && acc.numerator < acc.denominator)
            .map(|(c, acc)| (*c, acc))
            .collect();
        worst_keys.sort_unstable_by_key(|(_, acc)| *acc);

        let worst_inner_w = info_chunks[1].width.saturating_sub(4) as usize;
        let worst_inner_h = info_chunks[1].height.saturating_sub(2) as usize;
        let worst_lines: Vec<Line> = worst_keys
            .iter()
            .take(worst_inner_h.min(5))
            .map(|(c, acc)| {
                let pct = f64::from(**acc) * 100.0;
                let line = format!("{} at {:.1}% accuracy", c, pct);
                // Fall back to "c {pct}%" when the long form overflows.
                let line = if line.chars().count() > worst_inner_w {
                    format!("{} {:.1}%", c, pct)
                } else {
                    line
                };
                Line::from(line)
            })
            .collect();
        let worst_text = Text::from(worst_lines);
        let worst = Paragraph::new(worst_text).block(
            Block::default()
                .title(Span::styled("Worst Keys", theme.title))
                .borders(Borders::ALL)
                .border_type(theme.border_type)
                .border_style(theme.results_worst_keys_border)
                .padding(ratatui::widgets::Padding::horizontal(1)),
        );
        worst.render(info_chunks[1], buf);

        let missed_inner_w = info_chunks[2].width.saturating_sub(4) as usize;
        let missed_inner_h = info_chunks[2].height.saturating_sub(2) as usize;
        let missed_lines: Vec<Line> = self
            .missed_words
            .iter()
            .take(missed_inner_h)
            .map(|(w, count)| {
                let line = if *count > 1 {
                    format!("{} (x{})", w, count)
                } else {
                    w.clone()
                };
                Line::from(truncate_with_ellipsis(&line, missed_inner_w))
            })
            .collect();
        let missed_text = Text::from(missed_lines);
        let missed = Paragraph::new(missed_text).block(
            Block::default()
                .title(Span::styled("Missed Words", theme.title))
                .borders(Borders::ALL)
                .border_type(theme.border_type)
                .border_style(theme.results_missed_words_border)
                .padding(ratatui::widgets::Padding::horizontal(1)),
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
        let wpm_sma: Vec<(f64, f64)> = wpm_sma_full.iter().step_by(step).copied().collect();

        // Plot a point on the SMA curve for each missed word
        let missed: std::collections::HashSet<usize> = self
            .timing
            .missed_word_event_indices
            .iter()
            .copied()
            .collect();
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

            let mut wpm_datasets = vec![
                Dataset::default()
                    .name("WPM")
                    .marker(Marker::Braille)
                    .graph_type(GraphType::Line)
                    .style(theme.results_chart)
                    .data(&wpm_sma),
            ];

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

            let total_secs = self.timing.per_event.iter().sum::<f64>() as u64;
            let x_labels = vec![
                Span::raw("0:00"),
                Span::raw(format!("{}:{:02}", total_secs / 60, total_secs % 60)),
            ];

            let wpm_chart = Chart::new(wpm_datasets)
                .block(Block::default().title(vec![Span::styled("Chart", theme.title)]))
                .x_axis(
                    Axis::default()
                        .title(Span::raw(""))
                        .bounds([0.0, self.timing.per_event.len() as f64])
                        .labels(x_labels),
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
                                .collect::<Vec<_>>(),
                        ),
                );
            wpm_chart.render(res_chunks[1], buf);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod control_packing {
        use super::*;

        fn ctrl(label: char, desc: &'static str) -> Control {
            Control {
                label,
                desc,
                right_style: false,
            }
        }

        // Item widths: KEY_W(7) + 1 + desc.len()
        // quit=12, continue=16, repeat=14, "new test"=16, "main menu"=17,
        // "practice missed words"=29.

        #[test]
        fn fits_one_row_when_wide() {
            let controls = [
                ctrl('Q', "quit"),
                ctrl('C', "continue"),
                ctrl('R', "new test"),
                ctrl('M', "main menu"),
                ctrl('P', "practice missed words"),
            ];
            assert_eq!(pack_control_rows(&controls, 200), vec![5]);
            assert_eq!(control_art_rows(&controls, 200), 1);
        }

        #[test]
        fn wraps_when_too_narrow_for_one_row() {
            // Width 80: Q(12)+3+C(16)+3+R(16)+3+M(17) = 70 fits, +3+P(29)=102 wraps.
            let controls = [
                ctrl('Q', "quit"),
                ctrl('C', "continue"),
                ctrl('R', "new test"),
                ctrl('M', "main menu"),
                ctrl('P', "practice missed words"),
            ];
            assert_eq!(pack_control_rows(&controls, 80), vec![4, 1]);
            assert_eq!(control_art_rows(&controls, 80), 2);
        }

        #[test]
        fn returns_zero_rows_when_widest_doesnt_fit() {
            let controls = [ctrl('Q', "quit"), ctrl('P', "practice missed words")];
            // Widest tile is 29; width 20 cannot hold it.
            assert_eq!(control_art_rows(&controls, 20), 0);
        }

        #[test]
        fn empty_controls_zero_rows() {
            assert_eq!(control_art_rows(&[], 100), 0);
        }
    }

    mod split_words {
        use super::Status::*;
        use super::*;

        struct TestCase {
            word: &'static str,
            progress: &'static str,
            expected: Vec<(&'static str, Status)>,
        }

        fn setup(test_case: TestCase) -> (TestWord, &'static str, Vec<(String, Status)>) {
            let text = test_case.word;
            let word = TestWord {
                range: 0..text.len() as u32,
                target: text.to_string().into_boxed_str(),
                progress: test_case.progress.to_string(),
                events: Vec::new(),
            };
            let expected = test_case
                .expected
                .iter()
                .map(|(s, v)| (s.to_string(), *v))
                .collect::<Vec<_>>();
            (word, text, expected)
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
                let (word, text, expected) = setup(case);
                let got = split_typed_word(&word, text, false);
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
                let (word, text, expected) = setup(case);
                let got = split_current_word(&word, text, false);
                assert_eq!(got, expected);
            }
        }

        #[test]
        fn typed_word_ascii_skips_unicode() {
            // Word "café" typed as "caf" - the é is shown as Skipped (yellow)
            let text = "café";
            let word = TestWord {
                range: 0..text.len() as u32,
                target: "caf".to_string().into_boxed_str(),
                progress: "caf".to_string(),
                events: Vec::new(),
            };

            let got = split_typed_word(&word, text, true);
            let expected = vec![("caf".to_string(), Correct), ("é".to_string(), Skipped)];
            assert_eq!(got, expected);
        }

        #[test]
        fn current_word_ascii_skips_unicode() {
            // Word "café", user has typed "ca", cursor should be on 'f'
            let text = "café";
            let word = TestWord {
                range: 0..text.len() as u32,
                target: "caf".to_string().into_boxed_str(),
                progress: "ca".to_string(),
                events: Vec::new(),
            };

            let got = split_current_word(&word, text, true);
            let expected = vec![
                ("ca".to_string(), CurrentCorrect),
                ("f".to_string(), Cursor),
                ("é".to_string(), Skipped),
            ];
            assert_eq!(got, expected);
        }
    }
}
