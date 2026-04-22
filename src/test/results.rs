use super::{Test, is_missed_word_event};

use crossterm::event::KeyEvent;
use std::collections::HashMap;
use std::{cmp, fmt};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Fraction {
    pub numerator: usize,
    pub denominator: usize,
}

impl Fraction {
    pub const fn new(numerator: usize, denominator: usize) -> Self {
        Self {
            numerator,
            denominator,
        }
    }
}

impl From<Fraction> for f64 {
    fn from(f: Fraction) -> Self {
        f.numerator as f64 / f.denominator as f64
    }
}

impl cmp::Ord for Fraction {
    fn cmp(&self, other: &Self) -> cmp::Ordering {
        f64::from(*self).partial_cmp(&f64::from(*other)).unwrap()
    }
}

impl PartialOrd for Fraction {
    fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for Fraction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.numerator, self.denominator)
    }
}

pub struct TimingData {
    // Instead of storing WPM, we store CPS (clicks per second)
    pub overall_cps: f64,
    pub per_event: Vec<f64>,
    pub missed_word_event_indices: Vec<usize>,
    pub per_key: HashMap<KeyEvent, f64>,
}

pub struct AccuracyData {
    pub overall: Fraction,
    /// Per-target-character accuracy. A press contributes here iff a
    /// target character existed at the cursor position; it counts as
    /// correct iff the pressed char equals that target.
    pub per_key: HashMap<char, Fraction>,
}

pub struct Results {
    pub timing: TimingData,
    pub accuracy: AccuracyData,
    pub missed_words: Vec<(String, usize)>,
    pub is_repeat: bool,
    pub completed: bool,
}

impl From<&Test> for Results {
    fn from(test: &Test) -> Self {
        let events: Vec<&super::TestEvent> =
            test.words.iter().flat_map(|w| w.events.iter()).collect();

        // Track which event indices mark the end of a missed word.
        // The per_event array uses windows(2) so index i corresponds to
        // events[i+1]. We record the per_event index of the last event
        // in each word that had mistakes.
        let mut missed_indices = Vec::new();
        let mut event_offset: usize = 0;
        for word in &test.words {
            let word_len = word.events.len();
            if word_len > 0 && word.events.iter().any(is_missed_word_event) {
                // last event of this word is at event_offset + word_len - 1,
                // which maps to per_event index (event_offset + word_len - 2)
                // since per_event uses windows(2) starting from index 0
                let last_event = event_offset + word_len - 1;
                if last_event > 0 {
                    missed_indices.push(last_event - 1);
                }
            }
            event_offset += word_len;
        }

        let mut timing = calc_timing(&events);
        timing.missed_word_event_indices = missed_indices;

        Self {
            timing,
            accuracy: calc_accuracy(&events),
            missed_words: calc_missed_words(test),
            is_repeat: false,
            completed: test.complete,
        }
    }
}

fn calc_timing(events: &[&super::TestEvent]) -> TimingData {
    let mut timing = TimingData {
        overall_cps: -1.0,
        per_event: Vec::new(),
        missed_word_event_indices: Vec::new(),
        per_key: HashMap::new(),
    };

    // map of keys to a two-tuple (total time, clicks) for counting average
    let mut keys: HashMap<KeyEvent, (f64, usize)> = HashMap::new();

    for win in events.windows(2) {
        let event_dur = win[1]
            .time
            .checked_duration_since(win[0].time)
            .map(|d| d.as_secs_f64());

        if let Some(event_dur) = event_dur {
            timing.per_event.push(event_dur);

            let key = keys.entry(win[1].key).or_insert((0.0, 0));
            key.0 += event_dur;
            key.1 += 1;
        }
    }

    timing.per_key = keys
        .into_iter()
        .map(|(key, (total, count))| (key, total / count as f64))
        .collect();

    timing.overall_cps = timing.per_event.len() as f64 / timing.per_event.iter().sum::<f64>();

    timing
}

fn calc_accuracy(events: &[&super::TestEvent]) -> AccuracyData {
    let mut acc = AccuracyData {
        overall: Fraction::new(0, 0),
        per_key: HashMap::new(),
    };

    for event in events {
        if let Some(correct) = event.correct {
            acc.overall.denominator += 1;
            if correct {
                acc.overall.numerator += 1;
            }
        }

        if let (Some(target), crossterm::event::KeyCode::Char(pressed)) =
            (event.target, event.key.code)
        {
            let bucket = acc
                .per_key
                .entry(target)
                .or_insert_with(|| Fraction::new(0, 0));
            bucket.denominator += 1;
            if pressed == target {
                bucket.numerator += 1;
            }
        }
    }

    acc
}

fn calc_missed_words(test: &Test) -> Vec<(String, usize)> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    for word in &test.words {
        if word.events.iter().any(is_missed_word_event) {
            let count = counts.entry(word.text.clone()).or_insert_with(|| {
                order.push(word.text.clone());
                0
            });
            *count += 1;
        }
    }
    let mut result: Vec<_> = order
        .into_iter()
        .map(|w| {
            let count = counts[&w];
            (w, count)
        })
        .collect();
    result.sort_by(|a, b| b.1.cmp(&a.1));
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test::{Test, TestEvent};
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
    use std::time::{Duration, Instant};

    fn key(c: char) -> KeyEvent {
        KeyEvent {
            code: KeyCode::Char(c),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    fn event(time: Instant, c: char, correct: Option<bool>) -> TestEvent {
        TestEvent {
            time,
            key: key(c),
            correct,
            target: None,
        }
    }

    fn event_with_target(
        time: Instant,
        pressed: char,
        target: char,
        correct: Option<bool>,
    ) -> TestEvent {
        TestEvent {
            time,
            key: key(pressed),
            correct,
            target: Some(target),
        }
    }

    fn make_test(words: &[&str], ascii: bool) -> Test {
        Test::new(
            words.iter().map(|s| s.to_string()).collect(),
            true,
            false,
            true,
            Vec::new(),
            ascii,
            String::new(),
        )
    }

    #[test]
    fn fraction_ord_compares_values_not_fields() {
        // PartialEq is derived (field-wise), but Ord compares numeric value.
        assert_eq!(
            Fraction::new(1, 2).cmp(&Fraction::new(50, 100)),
            cmp::Ordering::Equal
        );
        assert!(Fraction::new(1, 2) < Fraction::new(2, 3));
        assert!(Fraction::new(3, 4) > Fraction::new(2, 3));
    }

    #[test]
    fn calc_timing_per_event_durations() {
        let base = Instant::now();
        let events = vec![
            event(base, 'a', Some(true)),
            event(base + Duration::from_millis(100), 'b', Some(true)),
            event(base + Duration::from_millis(300), 'c', Some(true)),
        ];
        let refs: Vec<&TestEvent> = events.iter().collect();
        let timing = calc_timing(&refs);
        assert_eq!(timing.per_event.len(), 2);
        assert!((timing.per_event[0] - 0.1).abs() < 1e-6);
        assert!((timing.per_event[1] - 0.2).abs() < 1e-6);
    }

    #[test]
    fn calc_timing_overall_cps() {
        let base = Instant::now();
        // 3 events, 1s apart → 2 intervals totalling 2s → cps = 1.0
        let events = vec![
            event(base, 'a', Some(true)),
            event(base + Duration::from_secs(1), 'b', Some(true)),
            event(base + Duration::from_secs(2), 'c', Some(true)),
        ];
        let refs: Vec<&TestEvent> = events.iter().collect();
        let timing = calc_timing(&refs);
        assert!((timing.overall_cps - 1.0).abs() < 1e-6);
    }

    #[test]
    fn calc_accuracy_counts_correct_vs_incorrect() {
        let base = Instant::now();
        let events = vec![
            event(base, 'a', Some(true)),
            event(base, 'b', Some(false)),
            event(base, 'c', Some(true)),
        ];
        let refs: Vec<&TestEvent> = events.iter().collect();
        let acc = calc_accuracy(&refs);
        assert_eq!(acc.overall, Fraction::new(2, 3));
    }

    #[test]
    fn calc_accuracy_buckets_by_target_not_pressed_key() {
        // User pressed 'd' when target was 's'. The mistake should be
        // attributed to 's', not 'd'.
        let base = Instant::now();
        let events = vec![
            event_with_target(base, 'd', 's', Some(false)),
            event_with_target(base, 's', 's', Some(true)),
        ];
        let refs: Vec<&TestEvent> = events.iter().collect();
        let acc = calc_accuracy(&refs);
        assert!(acc.per_key.get(&'d').is_none(), "'d' should not bucket");
        assert_eq!(acc.per_key.get(&'s'), Some(&Fraction::new(1, 2)));
    }

    #[test]
    fn calc_accuracy_mid_word_slip_does_not_penalise_later_correct_chars() {
        // Target "cat"; user types 'c','d','t' without backspacing.
        // Progress "cdt" never starts "cat" so the 't' event has correct=false,
        // but the 't' press itself matched its target 't' and should count
        // as 1/1, not 0/1.
        let base = Instant::now();
        let events = vec![
            event_with_target(base, 'c', 'c', Some(true)),
            event_with_target(base, 'd', 'a', Some(false)),
            event_with_target(base, 't', 't', Some(false)),
        ];
        let refs: Vec<&TestEvent> = events.iter().collect();
        let acc = calc_accuracy(&refs);
        assert_eq!(acc.per_key.get(&'c'), Some(&Fraction::new(1, 1)));
        assert_eq!(acc.per_key.get(&'a'), Some(&Fraction::new(0, 1)));
        assert_eq!(acc.per_key.get(&'t'), Some(&Fraction::new(1, 1)));
    }

    #[test]
    fn calc_accuracy_skips_none_correct_events() {
        // Ctrl-w / Ctrl-h push events with correct=None; they must not affect the ratio.
        let base = Instant::now();
        let events = vec![
            event(base, 'a', Some(true)),
            event(base, 'w', None),
            event(base, 'b', Some(false)),
        ];
        let refs: Vec<&TestEvent> = events.iter().collect();
        let acc = calc_accuracy(&refs);
        assert_eq!(acc.overall, Fraction::new(1, 2));
    }

    #[test]
    fn calc_missed_words_empty_when_all_correct() {
        let mut test = make_test(&["hello"], false);
        let base = Instant::now();
        test.words[0].events = vec![
            event(base, 'h', Some(true)),
            event(base, 'e', Some(true)),
            event(base, 'l', Some(true)),
            event(base, 'l', Some(true)),
            event(base, 'o', Some(true)),
        ];
        assert!(calc_missed_words(&test).is_empty());
    }

    #[test]
    fn calc_missed_words_dedups_counts_and_sorts_desc() {
        let mut test = make_test(&["hello", "world", "world"], false);
        let base = Instant::now();
        test.words[0].events = vec![event(base, 'h', Some(false))];
        test.words[1].events = vec![event(base, 'w', Some(false))];
        test.words[2].events = vec![event(base, 'w', Some(false))];
        let missed = calc_missed_words(&test);
        assert_eq!(
            missed,
            vec![("world".to_string(), 2), ("hello".to_string(), 1)]
        );
    }

    #[test]
    fn results_from_computes_missed_word_event_index() {
        // Word 1 has a mistake; its last event at absolute index 5 maps to
        // per_event index 4 (windows(2) drops one; last_event-1).
        let mut test = make_test(&["aa", "bb"], false);
        let base = Instant::now();
        test.words[0].events = vec![
            event(base, 'a', Some(true)),
            event(base + Duration::from_millis(100), 'a', Some(true)),
            event(base + Duration::from_millis(200), ' ', Some(true)),
        ];
        test.words[1].events = vec![
            event(base + Duration::from_millis(300), 'b', Some(false)),
            event(base + Duration::from_millis(400), 'b', Some(true)),
            event(base + Duration::from_millis(500), ' ', Some(true)),
        ];
        let results = Results::from(&test);
        assert_eq!(results.timing.missed_word_event_indices, vec![4]);
    }

}
