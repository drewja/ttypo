use super::{is_missed_word_event, Test};

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
    pub per_key: HashMap<KeyEvent, Fraction>,
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

    events
        .iter()
        .filter(|event| event.correct.is_some())
        .for_each(|event| {
            let key = acc
                .per_key
                .entry(event.key)
                .or_insert_with(|| Fraction::new(0, 0));

            acc.overall.denominator += 1;
            key.denominator += 1;

            if event.correct.unwrap() {
                acc.overall.numerator += 1;
                key.numerator += 1;
            }
        });

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
    let mut result: Vec<_> = order.into_iter().map(|w| {
        let count = counts[&w];
        (w, count)
    }).collect();
    result.sort_by(|a, b| b.1.cmp(&a.1));
    result
}
