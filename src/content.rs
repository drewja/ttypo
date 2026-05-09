//! Shared read-only text buffer plus pre-parsed word ranges / line layout.
//!
//! Files are memory-mapped when possible so large documents don't pay the
//! kernel→user copy, and word tokens borrow directly out of the mapping.
//! If a token contains non-whitespace control characters (rare), we fall
//! back to an owned, sanitized copy so the range-based tokens stay valid.

use crate::test::DisplayLine;

use memmap2::Mmap;
use std::fs::File;
use std::io;
use std::ops::Range;
use std::path::Path;

enum Backing {
    Mmap(Mmap),
    Owned(String),
}

impl std::fmt::Debug for Backing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Backing::Mmap(m) => f.debug_tuple("Mmap").field(&m.len()).finish(),
            Backing::Owned(s) => f.debug_tuple("Owned").field(&s.len()).finish(),
        }
    }
}

#[derive(Debug)]
pub struct Content {
    backing: Backing,
    pub word_ranges: Vec<Range<u32>>,
    pub lines: Vec<DisplayLine>,
    // Display label for the source (file path, "stdin", language name, ...).
    // Read via Content::source_label; not pub so consumers like Test::source
    // can override it (e.g. "practice" mode) without underlying Content drift.
    source_label: String,
}

impl Content {
    pub fn as_str(&self) -> &str {
        match &self.backing {
            // SAFETY: UTF-8 is validated once at construction and we never
            // mutate `backing` afterward. This relies on the file on disk
            // remaining untouched for the life of the mapping. If another
            // process rewrites it with non-UTF-8 bytes between validation
            // and this read, the unchecked conversion is unsound. For a
            // local typing-test tool we accept that risk.
            Backing::Mmap(m) => unsafe { std::str::from_utf8_unchecked(&m[..]) },
            Backing::Owned(s) => s.as_str(),
        }
    }

    pub fn as_bytes(&self) -> &[u8] {
        match &self.backing {
            Backing::Mmap(m) => &m[..],
            Backing::Owned(s) => s.as_bytes(),
        }
    }

    pub fn source_label(&self) -> &str {
        &self.source_label
    }

    #[cfg(test)]
    pub fn word(&self, idx: usize) -> &str {
        let r = &self.word_ranges[idx];
        &self.as_str()[r.start as usize..r.end as usize]
    }

    pub fn word_count(&self) -> usize {
        self.word_ranges.len()
    }

    pub fn is_empty(&self) -> bool {
        self.word_ranges.is_empty()
    }

    /// Load a file via mmap. Falls back to an owned sanitized copy if tokens
    /// contain control characters that must be stripped.
    pub fn from_file(path: &Path, source_label: String) -> io::Result<Self> {
        let file = File::open(path)?;
        let len = file.metadata()?.len();
        // Word ranges use u32 offsets; anything past u32::MAX would silently
        // wrap when ranges are computed. Reject up front with a clear error.
        if len > u32::MAX as u64 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "file is {} bytes; ttypo supports files up to {} bytes (4 GiB)",
                    len,
                    u32::MAX,
                ),
            ));
        }
        if len == 0 {
            return Ok(Self {
                backing: Backing::Owned(String::new()),
                word_ranges: Vec::new(),
                lines: Vec::new(),
                source_label,
            });
        }
        // SAFETY: mmap has two inherent hazards that Rust can't rule out:
        //   1. Concurrent writers can change the mapped bytes under us, so
        //      the UTF-8 validation below is only a point-in-time check:
        //      later reads via `as_str()` may see invalid UTF-8.
        //   2. If the file is truncated or unmapped while we hold the
        //      mapping, touching an evicted page raises SIGBUS (process
        //      dies, not a recoverable Rust error).
        // For a single-user typing-test tool operating on local files we
        // accept both risks; the alternative is a full fs::read copy.
        let mmap = unsafe { Mmap::map(&file)? };
        std::str::from_utf8(&mmap[..])
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let text = unsafe { std::str::from_utf8_unchecked(&mmap[..]) };
        if let Some((word_ranges, lines)) = try_parse_zero_copy(text) {
            return Ok(Self {
                backing: Backing::Mmap(mmap),
                word_ranges,
                lines,
                source_label,
            });
        }
        Ok(Self::from_text(text.to_string(), source_label))
    }

    /// Build from an owned String (stdin, sanitized fallback, etc.).
    pub fn from_text(text: String, source_label: String) -> Self {
        let (buf, word_ranges, lines) = parse_owned(&text);
        Self {
            backing: Backing::Owned(buf),
            word_ranges,
            lines,
            source_label,
        }
    }

    /// Build from an already-tokenized word list (language mode, practice
    /// mode, and tests). Words are concatenated into a single buffer with
    /// space separators so ranges remain valid.
    pub fn from_word_list<I, S>(words: I, source_label: String) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut buf = String::new();
        let mut word_ranges = Vec::new();
        for w in words {
            let w = w.as_ref();
            if w.is_empty() {
                continue;
            }
            let start = buf.len() as u32;
            buf.push_str(w);
            let end = buf.len() as u32;
            word_ranges.push(start..end);
            buf.push(' ');
        }
        Self {
            backing: Backing::Owned(buf),
            word_ranges,
            lines: Vec::new(),
            source_label,
        }
    }
}

// Zero-copy parse: tokens borrow byte ranges directly out of `text`.
// Returns None if any token contains control characters that would otherwise
// be stripped, in which case the caller falls back to `parse_owned`.
fn try_parse_zero_copy(text: &str) -> Option<(Vec<Range<u32>>, Vec<DisplayLine>)> {
    let base = text.as_ptr() as usize;
    let mut word_ranges: Vec<Range<u32>> = Vec::new();
    let mut lines: Vec<DisplayLine> = Vec::new();

    for line in text.lines() {
        let indent: String = line
            .chars()
            .take_while(|c| c.is_whitespace())
            .collect::<String>()
            .replace('\t', "    ");

        let word_start = word_ranges.len();
        for token in line.split_whitespace() {
            if token.chars().any(|c| c.is_control()) {
                return None;
            }
            let start = (token.as_ptr() as usize - base) as u32;
            let end = start + token.len() as u32;
            word_ranges.push(start..end);
        }
        let word_count = word_ranges.len() - word_start;
        lines.push(DisplayLine {
            indent,
            word_start,
            word_count,
        });
    }

    Some((word_ranges, lines))
}

// Owned parse: sanitize tokens (strip control chars) and produce a fresh
// backing buffer whose ranges match the sanitized text. The buffer only
// needs to hold word bytes (nothing slices newlines or indentation out of
// it), so we just concatenate sanitized tokens with single-space separators.
// DisplayLine::indent carries the display-side whitespace independently.
fn parse_owned(text: &str) -> (String, Vec<Range<u32>>, Vec<DisplayLine>) {
    let mut buf = String::with_capacity(text.len());
    let mut word_ranges: Vec<Range<u32>> = Vec::new();
    let mut lines: Vec<DisplayLine> = Vec::new();

    for line in text.lines() {
        let indent: String = line
            .chars()
            .take_while(|c| c.is_whitespace())
            .collect::<String>()
            .replace('\t', "    ");

        let word_start = word_ranges.len();
        for token in line.split_whitespace() {
            let sanitized_start = buf.len();
            if !buf.is_empty() {
                buf.push(' ');
            }
            let start = buf.len() as u32;
            for c in token.chars() {
                if !c.is_control() {
                    buf.push(c);
                }
            }
            let end = buf.len() as u32;
            if end == start {
                // Token was entirely control chars; undo the separator too.
                buf.truncate(sanitized_start);
                continue;
            }
            word_ranges.push(start..end);
        }
        let word_count = word_ranges.len() - word_start;
        lines.push(DisplayLine {
            indent,
            word_start,
            word_count,
        });
    }

    (buf, word_ranges, lines)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn from_word_list_ranges_slice_correctly() {
        let c = Content::from_word_list(["hello", "world", "rust"], String::new());
        assert_eq!(c.word_count(), 3);
        assert_eq!(c.word(0), "hello");
        assert_eq!(c.word(1), "world");
        assert_eq!(c.word(2), "rust");
    }

    #[test]
    fn from_word_list_skips_empty() {
        let c = Content::from_word_list(["a", "", "b"], String::new());
        assert_eq!(c.word_count(), 2);
        assert_eq!(c.word(0), "a");
        assert_eq!(c.word(1), "b");
    }

    #[test]
    fn from_text_preserves_unicode() {
        let c = Content::from_text("héllo wörld".to_string(), String::new());
        assert_eq!(c.word_count(), 2);
        assert_eq!(c.word(0), "héllo");
        assert_eq!(c.word(1), "wörld");
    }

    #[test]
    fn from_text_tracks_line_layout() {
        let c = Content::from_text("a b\nc\n".to_string(), String::new());
        assert_eq!(c.lines.len(), 2);
        assert_eq!(c.lines[0].word_start, 0);
        assert_eq!(c.lines[0].word_count, 2);
        assert_eq!(c.lines[1].word_start, 2);
        assert_eq!(c.lines[1].word_count, 1);
    }

    #[test]
    fn from_text_expands_tabs_in_indent() {
        let c = Content::from_text("\thello".to_string(), String::new());
        assert_eq!(c.lines.len(), 1);
        assert_eq!(c.lines[0].indent, "    ");
    }

    #[test]
    fn from_file_mmap_zero_copy() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("src.txt");
        let mut f = File::create(&path).unwrap();
        f.write_all(b"hello world\nfoo bar baz\n").unwrap();
        drop(f);

        let c = Content::from_file(&path, "src.txt".to_string()).unwrap();
        assert_eq!(c.word_count(), 5);
        assert_eq!(c.word(0), "hello");
        assert_eq!(c.word(4), "baz");
        assert_eq!(c.lines.len(), 2);
    }

    #[test]
    fn from_file_empty_returns_empty_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.txt");
        File::create(&path).unwrap();
        let c = Content::from_file(&path, "empty".to_string()).unwrap();
        assert!(c.is_empty());
        assert_eq!(c.word_count(), 0);
    }

    #[test]
    fn from_file_invalid_utf8_errors() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.txt");
        let mut f = File::create(&path).unwrap();
        f.write_all(&[0xFF, 0xFE, 0xFD]).unwrap();
        drop(f);
        assert!(Content::from_file(&path, "bad".to_string()).is_err());
    }

    #[test]
    fn from_file_with_control_char_falls_back_to_owned() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ctrl.txt");
        let mut f = File::create(&path).unwrap();
        // Non-whitespace control (BEL, 0x07) embedded in a token.
        f.write_all(b"hello\x07there world\n").unwrap();
        drop(f);

        let c = Content::from_file(&path, "ctrl".to_string()).unwrap();
        // "hello\x07there" becomes "hellothere" after stripping.
        assert_eq!(c.word_count(), 2);
        assert_eq!(c.word(0), "hellothere");
        assert_eq!(c.word(1), "world");
    }
}
