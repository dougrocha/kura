//! Captions: the text written inside an image.
//!
//! Domain types only. SQL lives in `store`, matching in `search`.
//!
//! Why not `model::clean_text`: that rule rejects control characters, and OCR
//! returns one string per line. A caption is untrusted machine output, so the
//! rule is "normalize, never reject": a read that produced garbage must still
//! be recorded, or the image would be read again forever.

use std::{fmt, str::FromStr};

use crate::error::InvalidInput;

/// Longest stored caption, in chars. Longer text is cut at a word boundary.
pub const CAPTION_MAX_CHARS: usize = 1000;

/// How far back from the cut a space may be found to end the text on a word.
const WORD_CUT_WINDOW: usize = 40;

const ENGINE_MAX_CHARS: usize = 32;

/// Caption text. The only constructor, [`CaptionText::normalize`], guarantees:
/// non-empty, trimmed, whitespace and control runs collapsed to one space, at
/// most [`CAPTION_MAX_CHARS`] chars. Case and accents are kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptionText(String);

impl CaptionText {
    /// Normalize raw OCR output or hand-typed text. Never fails: text with
    /// nothing left in it is [`Outcome::Blank`].
    pub fn normalize(raw: &str) -> Outcome {
        let mut out = String::with_capacity(raw.len().min(CAPTION_MAX_CHARS * 4));
        let mut pending_space = false;
        for c in raw.chars() {
            if c.is_whitespace() || c.is_control() {
                pending_space = !out.is_empty();
            } else {
                if pending_space {
                    out.push(' ');
                    pending_space = false;
                }
                out.push(c);
            }
        }

        if let Some((cut, _)) = out.char_indices().nth(CAPTION_MAX_CHARS) {
            let head = &out[..cut];
            let at_word_end = out[cut..].starts_with(' ');
            let keep = if at_word_end {
                head
            } else {
                let window_start = head.char_indices().rev().nth(WORD_CUT_WINDOW).map_or(0, |(i, _)| i);
                head[window_start..].rfind(' ').map_or(head, |space| &head[..window_start + space])
            };
            out.truncate(keep.trim_end().len());
        }

        if out.is_empty() { Outcome::Blank } else { Outcome::Text(Self(out)) }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Rows are trusted: they were normalized on the way in.
    pub(crate) fn from_row(text: String) -> Self {
        Self(text)
    }
}

impl fmt::Display for CaptionText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// What a read of an image found. Makes "looked, found nothing" a value
/// instead of an empty string callers must remember to special-case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Read, and there is no text. Stored as `text = ''`.
    Blank,
    Text(CaptionText),
}

impl Outcome {
    /// `""` for [`Outcome::Blank`]. For display and the wire, not for logic.
    pub fn text(&self) -> &str {
        match self {
            Self::Blank => "",
            Self::Text(t) => t.as_str(),
        }
    }

    pub(crate) fn from_row(text: String) -> Self {
        if text.is_empty() { Self::Blank } else { Self::Text(CaptionText::from_row(text)) }
    }
}

/// Who wrote a caption. Closed set, mirrored by the table's CHECK.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptionSource {
    Ocr,
    Manual,
}

impl CaptionSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ocr => "ocr",
            Self::Manual => "manual",
        }
    }
}

impl FromStr for CaptionSource {
    type Err = InvalidInput;

    fn from_str(s: &str) -> Result<Self, InvalidInput> {
        match s {
            "ocr" => Ok(Self::Ocr),
            "manual" => Ok(Self::Manual),
            _ => Err(InvalidInput::new("caption source", "expected 'ocr' or 'manual'")),
        }
    }
}

impl fmt::Display for CaptionSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Recognizer tag such as `vision:1`: 1 to 32 chars of `a-z 0-9 : . _ -`.
/// Opaque here. A client compares it with its own current engine to decide
/// whether an ocr caption is stale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Engine(String);

impl Engine {
    pub fn parse(s: &str) -> Result<Self, InvalidInput> {
        let valid = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, ':' | '.' | '_' | '-');
        if s.is_empty() || s.len() > ENGINE_MAX_CHARS || !s.chars().all(valid) {
            return Err(InvalidInput::new("engine", "expected 1 to 32 characters of a-z, 0-9, ':', '.', '_' or '-'"));
        }
        Ok(Self(s.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn from_row(s: String) -> Self {
        Self(s)
    }
}

impl fmt::Display for Engine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// An image's effective caption: the manual one if there is one, else the ocr
/// one. An ocr reading always has an engine and a manual one never does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reading {
    Ocr { engine: Engine, outcome: Outcome },
    Manual { outcome: Outcome },
}

impl Reading {
    pub fn outcome(&self) -> &Outcome {
        match self {
            Self::Ocr { outcome, .. } | Self::Manual { outcome } => outcome,
        }
    }

    pub fn source(&self) -> CaptionSource {
        match self {
            Self::Ocr { .. } => CaptionSource::Ocr,
            Self::Manual { .. } => CaptionSource::Manual,
        }
    }

    pub fn engine(&self) -> Option<&Engine> {
        match self {
            Self::Ocr { engine, .. } => Some(engine),
            Self::Manual { .. } => None,
        }
    }
}

/// The one caption write. Idempotent: writing what is already stored changes nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptionEdit {
    /// Replace the ocr row. Never touches the manual one.
    Ocr { outcome: Outcome, engine: Engine },
    /// Replace the manual row. `Outcome::Blank` means "there is no text here".
    Manual(Outcome),
    /// Delete one source's row. Clearing ocr means "read it again"; clearing
    /// manual falls back to what ocr read.
    Clear(CaptionSource),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(raw: &str) -> String {
        CaptionText::normalize(raw).text().to_owned()
    }

    #[test]
    fn whitespace_and_control_runs_become_one_space() {
        assert_eq!(text("  WHEN THE\nMOON\r\n\tHITS \u{a0} "), "WHEN THE MOON HITS");
        assert_eq!(text("a\u{0}b"), "a b");
        assert_eq!(text("naïve  CAFÉ"), "naïve CAFÉ");
    }

    #[test]
    fn nothing_left_is_blank() {
        for raw in ["", "   ", "\n\t", "\u{0}\u{7}"] {
            assert_eq!(CaptionText::normalize(raw), Outcome::Blank, "{raw:?}");
        }
    }

    #[test]
    fn long_text_is_cut_on_a_word_boundary() {
        let raw = "word ".repeat(400);
        let cut = text(&raw);
        assert!(cut.chars().count() <= CAPTION_MAX_CHARS);
        assert!(cut.ends_with("word"));
        assert!(cut.chars().count() > CAPTION_MAX_CHARS - WORD_CUT_WINDOW);

        let exact = "x".repeat(CAPTION_MAX_CHARS);
        assert_eq!(text(&exact), exact);

        let one_word = "y".repeat(CAPTION_MAX_CHARS + 50);
        assert_eq!(text(&one_word).chars().count(), CAPTION_MAX_CHARS);
    }

    #[test]
    fn cut_never_splits_a_multibyte_char() {
        let raw = "é".repeat(CAPTION_MAX_CHARS + 10);
        assert_eq!(text(&raw).chars().count(), CAPTION_MAX_CHARS);
    }

    #[test]
    fn sources_and_engines_parse() {
        assert_eq!("ocr".parse::<CaptionSource>().unwrap(), CaptionSource::Ocr);
        assert_eq!("manual".parse::<CaptionSource>().unwrap(), CaptionSource::Manual);
        assert!("OCR".parse::<CaptionSource>().is_err());
        assert!(Engine::parse("vision:1").is_ok());
        assert!(Engine::parse("").is_err());
        assert!(Engine::parse("Vision").is_err());
        assert!(Engine::parse(&"a".repeat(33)).is_err());
    }
}
