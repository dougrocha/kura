//! The one search engine: `Library::search`. The TUI calls it in-process and
//! `kura-server` exposes it as `GET /search`, so every client ranks the same.
//!
//! ```text
//! let results = library.search(&SearchQuery::parse("when moon")).await?;
//! for hit in &results.hits {
//!     hit.image();
//!     hit.snippet(); // only when the caption took part in the match
//! }
//! ```
//!
//! ## What is searched
//!
//! Each image has two fields, matched by different rules.
//!
//! * **meta** is `name + " " + tags.join(" ")`, matched by nucleo with fzf
//!   syntax, smart case and smart accents. `#word` keeps images with a tag that
//!   starts with `word` (ignoring case). This is how search worked before
//!   captions existed, and a query that only touches meta behaves the same.
//! * **caption** is the effective caption, folded: lowercase and accent
//!   stripped, one char for one char. OCR casing is unreliable, so captions
//!   ignore smart case and smart accents.
//!
//! ## How captions join without bending meta
//!
//! The caption is never appended to the nucleo haystack. Matching is AND across
//! words (atoms), and each atom can be satisfied by a different field.
//!
//! * A fuzzy atom is satisfied by meta (nucleo), or else by the caption when it
//!   is a prefix of some caption word. It is never a fuzzy subsequence over
//!   caption text: long text satisfies almost any subsequence.
//! * A `'substring` atom is satisfied by meta, or else by the text appearing
//!   anywhere in the caption.
//! * Needles shorter than 2 chars skip the caption.
//! * `^prefix`, `suffix$` and `^exact$` atoms look at meta only.
//! * A `!word` atom rejects an image whose meta matches it (as before) or whose
//!   caption contains it. Anchored negatives look at meta only.
//!
//! ## Ranking
//!
//! `(caption_atoms ascending, score descending, library order)`. The first key
//! is how many atoms needed the caption, so a name or tag hit always beats a
//! caption-only hit and no number is shared between nucleo and the caption
//! rules. The score is the nucleo score of the meta atoms plus a caption score
//! (whole word, then word prefix, then substring). Ties keep the library's
//! newest-first order.

use std::{cmp::Reverse, sync::Arc};

use nucleo_matcher::{
    Config, Matcher, Utf32Str,
    chars::{normalize, to_lower_case},
    pattern::{AtomKind, CaseMatching, Normalization, Pattern},
};

use crate::{
    Image,
    changes::ChangeSeq,
    error::{Result, ResultExt},
    store::{Query, Store},
};

/// A parsed search. Parsing can't fail: any text is a query, and an empty one
/// matches every image.
#[derive(Clone)]
pub struct SearchQuery {
    /// Lowercase, from `#word`.
    tag_prefixes: Vec<String>,
    pattern: Pattern,
    /// One per `pattern.atoms`: what the caption makes of the same atom.
    caption_atoms: Vec<CaptionAtom>,
}

#[derive(Clone)]
struct CaptionAtom {
    /// Folded needle.
    needle: Vec<char>,
    /// Where a positive atom may match in the caption. `None` when the atom
    /// never consults the caption.
    positive: Option<Place>,
    /// A negative atom consults the caption as a substring.
    negative: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Place {
    WordStart,
    Anywhere,
}

/// Needles shorter than this skip the caption.
const MIN_CAPTION_NEEDLE: usize = 2;

impl SearchQuery {
    pub fn parse(input: &str) -> Self {
        let mut tag_prefixes = vec![];
        let mut words = vec![];
        for word in input.split_whitespace() {
            match word.strip_prefix('#') {
                Some("") => {}
                Some(tag) => tag_prefixes.push(tag.to_lowercase()),
                None => words.push(word),
            }
        }

        let pattern = Pattern::parse(&words.join(" "), CaseMatching::Smart, Normalization::Smart);
        let caption_atoms = pattern
            .atoms
            .iter()
            .map(|atom| {
                let needle: Vec<char> = atom.needle_text().chars().map(fold_char).collect();
                let consults = !needle.is_empty();
                CaptionAtom {
                    positive: match atom.kind {
                        _ if atom.negative || needle.len() < MIN_CAPTION_NEEDLE => None,
                        AtomKind::Fuzzy => Some(Place::WordStart),
                        AtomKind::Substring => Some(Place::Anywhere),
                        _ => None,
                    },
                    negative: atom.negative && atom.kind == AtomKind::Substring && consults,
                    needle,
                }
            })
            .collect();
        Self { tag_prefixes, pattern, caption_atoms }
    }

    fn is_empty(&self) -> bool {
        self.tag_prefixes.is_empty() && self.pattern.atoms.is_empty()
    }
}

/// The ranked answer: every matching image, best first.
#[derive(Debug)]
pub struct SearchResults {
    /// The change-log position this answer reflects. A client whose own copy of
    /// the library was loaded at an earlier seq knows it is out of date.
    pub seq: ChangeSeq,
    pub hits: Vec<Hit>,
    pub coverage: Coverage,
}

/// How much of the library has a caption, so "why isn't my meme found?" has an
/// answer: when `unread > 0`, some images are searchable by name and tags only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Coverage {
    pub images: usize,
    /// Images with no caption at all. One read and found empty counts as read.
    pub unread: usize,
}

#[derive(Debug)]
pub struct Hit {
    image: Arc<Image>,
    snippet: Option<Snippet>,
}

impl Hit {
    pub fn image(&self) -> &Image {
        &self.image
    }

    /// Present only when the caption took part in the match.
    pub fn snippet(&self) -> Option<&Snippet> {
        self.snippet.as_ref()
    }
}

/// A window of the caption around the first match, as segments, so no client
/// maps offsets between UTF-8, UTF-16 and graphemes. The segments' text joined
/// is a contiguous piece of the stored caption; the cut flags say whether text
/// before or after it was left out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snippet {
    pub leading_cut: bool,
    pub segments: Vec<Segment>,
    pub trailing_cut: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub text: String,
    pub matched: bool,
}

// ---------------------------------------------------------------- index

/// An immutable snapshot of everything searchable, taken at one change-log
/// position. Pure data and pure functions, so it is tested with hand-made images.
pub(crate) struct SearchIndex {
    seq: ChangeSeq,
    /// Library order, newest first: the last tiebreak.
    docs: Vec<Doc>,
    unread: usize,
}

struct Doc {
    image: Arc<Image>,
    meta: String,
    /// The tags lowercased, for `#tag` prefixes.
    tags_lower: Vec<String>,
    /// The caption folded char for char, empty when there is no text.
    caption: Vec<char>,
}

impl SearchIndex {
    pub(crate) fn build(seq: ChangeSeq, images: Vec<Image>) -> Self {
        let unread = images.iter().filter(|image| image.caption().is_none()).count();
        let docs = images
            .into_iter()
            .map(|image| {
                let meta = std::iter::once(image.name().as_str())
                    .chain(image.tags().iter().map(|t| t.as_str()))
                    .collect::<Vec<_>>()
                    .join(" ");
                let caption =
                    image.caption().map_or_else(Vec::new, |r| r.outcome().text().chars().map(fold_char).collect());
                let tags_lower = image.tags().iter().map(|t| t.as_str().to_lowercase()).collect();
                Doc { image: Arc::new(image), meta, tags_lower, caption }
            })
            .collect();
        Self { seq, docs, unread }
    }

    pub(crate) fn query(&self, query: &SearchQuery) -> SearchResults {
        let coverage = Coverage { images: self.docs.len(), unread: self.unread };
        if query.is_empty() {
            let hits = self.docs.iter().map(|doc| Hit { image: doc.image.clone(), snippet: None }).collect();
            return SearchResults { seq: self.seq, hits, coverage };
        }

        let mut matcher = Matcher::new(Config::DEFAULT);
        let mut buf = vec![];
        let mut scored: Vec<(usize, Scored)> = self
            .docs
            .iter()
            .enumerate()
            .filter_map(|(offset, doc)| score_doc(query, doc, &mut matcher, &mut buf).map(|s| (offset, s)))
            .collect();
        scored.sort_by_key(|(offset, s)| (s.caption_atoms, Reverse(s.score), *offset));

        let hits = scored
            .into_iter()
            .map(|(offset, scored)| {
                let doc = &self.docs[offset];
                let snippet = (scored.caption_atoms > 0).then(|| snippet(&original_chars(&doc.image), &scored.spans));
                Hit { image: doc.image.clone(), snippet }
            })
            .collect();
        SearchResults { seq: self.seq, hits, coverage }
    }
}

struct Scored {
    /// Positive atoms that only the caption satisfied. The primary rank key.
    caption_atoms: u32,
    score: u32,
    /// Caption char ranges that matched, for the snippet.
    spans: Vec<(usize, usize)>,
}

fn score_doc(query: &SearchQuery, doc: &Doc, matcher: &mut Matcher, buf: &mut Vec<char>) -> Option<Scored> {
    let has_tags = query.tag_prefixes.iter().all(|wanted| doc.tags_lower.iter().any(|tag| tag.starts_with(wanted)));
    if !has_tags {
        return None;
    }

    let mut scored = Scored { caption_atoms: 0, score: 0, spans: vec![] };
    if query.pattern.atoms.is_empty() {
        return Some(scored);
    }

    let meta = Utf32Str::new(&doc.meta, buf);
    for (atom, caption) in query.pattern.atoms.iter().zip(&query.caption_atoms) {
        let meta_score = atom.score(meta, matcher);
        if atom.negative {
            // nucleo says `None` when the negated text is in meta.
            if meta_score.is_none() || (caption.negative && contains(&doc.caption, &caption.needle)) {
                return None;
            }
        } else if let Some(s) = meta_score {
            scored.score += u32::from(s);
        } else {
            let place = caption.positive?;
            let found = find(&doc.caption, &caption.needle, place)?;
            scored.caption_atoms += 1;
            scored.score += found.tier as u32 * 100 + caption.needle.len().min(50) as u32;
            scored.spans.push((found.at, found.at + caption.needle.len()));
        }
    }
    Some(scored)
}

/// How well the caption matched, strongest last.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Tier {
    Substring = 1,
    WordPrefix = 2,
    WholeWord = 3,
}

struct Found {
    tier: Tier,
    at: usize,
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric()
}

/// The best place `needle` occurs in `text`, earliest on ties. `Place::WordStart`
/// only counts occurrences that begin a word.
fn find(text: &[char], needle: &[char], place: Place) -> Option<Found> {
    let n = needle.len();
    if n == 0 || n > text.len() {
        return None;
    }
    let mut best: Option<Found> = None;
    for at in 0..=text.len() - n {
        if &text[at..at + n] != needle {
            continue;
        }
        let word_start = at == 0 || !is_word_char(text[at - 1]);
        if place == Place::WordStart && !word_start {
            continue;
        }
        let word_end = at + n == text.len() || !is_word_char(text[at + n]);
        let tier = match (word_start, word_end) {
            (true, true) => Tier::WholeWord,
            (true, false) => Tier::WordPrefix,
            _ => Tier::Substring,
        };
        if best.as_ref().is_none_or(|b| tier > b.tier) {
            if tier == Tier::WholeWord {
                return Some(Found { tier, at });
            }
            best = Some(Found { tier, at });
        }
    }
    best
}

fn contains(text: &[char], needle: &[char]) -> bool {
    needle.len() <= text.len() && text.windows(needle.len()).any(|w| w == needle)
}

/// Lowercase and strip the accent, always one char to one char, so a char index
/// into the folded caption is a char index into the stored one.
fn fold_char(c: char) -> char {
    to_lower_case(normalize(c))
}

// ---------------------------------------------------------------- snippets

const SNIPPET_LEAD: usize = 24;
const SNIPPET_MAX: usize = 80;

fn original_chars(image: &Image) -> Vec<char> {
    image.caption().map_or_else(Vec::new, |r| r.outcome().text().chars().collect())
}

/// A window of `text` around the first span, starting and ending on word
/// boundaries where there is one, with every span inside it marked matched.
fn snippet(text: &[char], spans: &[(usize, usize)]) -> Snippet {
    let mut spans = spans.to_vec();
    spans.sort_unstable();
    let mut merged: Vec<(usize, usize)> = vec![];
    for (start, end) in spans {
        match merged.last_mut() {
            Some(last) if start <= last.1 => last.1 = last.1.max(end),
            _ => merged.push((start, end)),
        }
    }
    let first = merged.first().copied().unwrap_or((0, 0));

    let mut start = first.0.saturating_sub(SNIPPET_LEAD);
    if start > 0
        && text[start - 1] != ' '
        && let Some(space) = text[start..first.0].iter().position(|&c| c == ' ')
    {
        start += space + 1;
    }
    let mut end = (start + SNIPPET_MAX).max(first.1).min(text.len());
    if end < text.len()
        && text[end] != ' '
        && let Some(space) = text[first.1..end].iter().rposition(|&c| c == ' ')
    {
        end = first.1 + space;
    }

    let slice = |from: usize, to: usize| text[from..to].iter().collect::<String>();
    let mut segments = vec![];
    let mut cursor = start;
    for (from, to) in merged {
        let (from, to) = (from.max(start), to.min(end));
        if from >= to {
            continue;
        }
        if from > cursor {
            segments.push(Segment { text: slice(cursor, from), matched: false });
        }
        segments.push(Segment { text: slice(from, to), matched: true });
        cursor = to;
    }
    if cursor < end {
        segments.push(Segment { text: slice(cursor, end), matched: false });
    }
    Snippet { leading_cut: start > 0, segments, trailing_cut: end < text.len() }
}

// ---------------------------------------------------------------- cache

/// Keeps one never-stale `SearchIndex`, deriving freshness from the change log
/// instead of syncing with writers:
///
/// 1. read the log's head;
/// 2. if the cached index is at least that new, reuse it;
/// 3. else rebuild from the database and stamp it with that head.
///
/// The head is read before the rebuild, so the stamp is never ahead of the data:
/// a write that lands mid-rebuild costs one extra rebuild next time, never a
/// stale answer. Writes from other processes count too, since the log lives in
/// the database. This relies on every write that changes searchable state
/// recording a change in its own transaction, which `Library` does for each.
pub(crate) struct SearchCache {
    /// Held across a rebuild, so concurrent searches share one.
    current: tokio::sync::Mutex<Option<Arc<SearchIndex>>>,
}

impl SearchCache {
    pub(crate) fn new() -> Self {
        Self { current: tokio::sync::Mutex::new(None) }
    }

    pub(crate) async fn fresh(&self, store: &Store) -> Result<Arc<SearchIndex>> {
        let head = store.head_seq().await?;
        let mut current = self.current.lock().await;
        if let Some(index) = current.as_ref().filter(|index| index.seq >= head) {
            return Ok(index.clone());
        }
        let images = store.images(Query::default()).await?;
        let index = tokio::task::spawn_blocking(move || Arc::new(SearchIndex::build(head, images)))
            .await
            .internal("building the search index")?;
        *current = Some(index.clone());
        Ok(index)
    }
}

#[cfg(test)]
mod tests {
    use jiff::Timestamp;
    use nucleo_matcher::Config;

    use super::*;
    use crate::{CaptionText, ImageHash, ImageName, Reading, Tag};

    fn image(n: usize, name: &str, tags: &[&str], caption: Option<&str>) -> Image {
        Image {
            hash: ImageHash::from_row(format!("{n:064x}")),
            name: ImageName::from_row(name.to_owned()),
            extension: "png".to_owned(),
            created_at: Timestamp::UNIX_EPOCH,
            tags: tags.iter().map(|t| Tag::from_row((*t).to_owned())).collect(),
            reading: caption.map(|text| Reading::Manual { outcome: CaptionText::normalize(text) }),
        }
    }

    fn run(query: &str, images: Vec<Image>) -> SearchResults {
        SearchIndex::build(ChangeSeq(0), images).query(&SearchQuery::parse(query))
    }

    fn matches(query: &str, name: &str, tags: &[&str], caption: Option<&str>) -> bool {
        !run(query, vec![image(1, name, tags, caption)]).hits.is_empty()
    }

    fn names(results: &SearchResults) -> Vec<&str> {
        results.hits.iter().map(|h| h.image().name().as_str()).collect()
    }

    fn order(query: &str, images: &[(&str, Option<&str>)]) -> Vec<String> {
        let images = images.iter().enumerate().map(|(i, (name, caption))| image(i, name, &[], *caption)).collect();
        names(&run(query, images)).into_iter().map(str::to_owned).collect()
    }

    // The meta rules, unchanged from before captions.

    #[test]
    fn empty_query_matches_everything() {
        assert!(SearchQuery::parse("").is_empty());
        assert!(SearchQuery::parse("  # ").is_empty());
        let results = run("", vec![image(1, "b", &[], None), image(2, "a", &[], Some("x"))]);
        assert_eq!(names(&results), ["b", "a"]);
        assert!(results.hits.iter().all(|h| h.snippet().is_none()));
        assert_eq!(results.coverage, Coverage { images: 2, unread: 1 });
    }

    #[test]
    fn fuzzy_matches_name() {
        assert!(matches("cat", "cat vibing", &[], None));
        assert!(matches("cvb", "cat vibing", &[], None));
        assert!(!matches("dog", "cat vibing", &[], None));
    }

    #[test]
    fn fuzzy_matches_tags() {
        assert!(matches("reaction", "cat vibing", &["reaction"], None));
    }

    #[test]
    fn hash_words_filter_by_tag_prefix() {
        assert!(matches("#reac", "cat vibing", &["Reaction"], None));
        assert!(!matches("#reac", "cat vibing", &["music"], None));
        assert!(matches("#reac #mus", "cat vibing", &["reaction", "music"], None));
        assert!(!matches("#reac #mus", "cat vibing", &["reaction"], None));
    }

    #[test]
    fn smart_case() {
        assert!(matches("cat", "Cat Jam", &[], None));
        assert!(!matches("Cat", "cat jam", &[], None));
        assert!(matches("Cat", "Cat jam", &[], None));
    }

    #[test]
    fn smart_accents() {
        assert!(matches("cafe", "café?", &[], None));
        assert!(!matches("café", "cafe", &[], None));
        assert!(matches("café", "café?", &[], None));
        assert!(matches("#cafe", "x", &["Cafe"], None));
        assert!(!matches("#café", "x", &["cafe"], None));
    }

    #[test]
    fn fzf_operators() {
        assert!(!matches("!cat", "cat jam", &[], None));
        assert!(matches("!cat", "dog", &[], None));
        assert!(!matches("!cat", "the cats", &[], None));
        assert!(matches("'at", "cat jam", &[], None));
        assert!(!matches("'cj", "cat jam", &[], None));
        assert!(matches("^cat", "cat jam", &[], None));
        assert!(!matches("^jam", "cat jam", &[], None));
        assert!(matches("jam$", "cat jam", &[], None));
        assert!(!matches("cat$", "cat jam", &[], None));
        assert!(matches("^cat$", "cat", &[], None));
        assert!(!matches("^cat$", "cat jam", &[], None));
        assert!(matches("\\!cat", "!cat", &[], None));
    }

    #[test]
    fn tag_filter_and_fuzzy_combine() {
        assert!(matches("#reaction cat", "cat vibing", &["reaction"], None));
        assert!(!matches("#reaction dog", "cat vibing", &["reaction"], None));
    }

    #[test]
    fn meta_only_queries_score_like_pattern_score() {
        let img = image(1, "cat vibing", &["reaction", "music"], None);
        for query in ["cat", "cvb reac", "'vib ^cat", "cat !dog", "reaction$ cat"] {
            let pattern = Pattern::parse(query, CaseMatching::Smart, Normalization::Smart);
            let mut matcher = Matcher::new(Config::DEFAULT);
            let expected = pattern.score(Utf32Str::new("cat vibing reaction music", &mut vec![]), &mut matcher);

            let index = SearchIndex::build(ChangeSeq(0), vec![img.clone()]);
            let q = SearchQuery::parse(query);
            let mut buf = vec![];
            let got = score_doc(&q, &index.docs[0], &mut matcher, &mut buf).map(|s| s.score);
            assert_eq!(got, expected, "{query}");
        }
    }

    // Captions.

    #[test]
    fn caption_words_find_the_image() {
        let boss = Some("WHEN THE BOSS WALKS IN");
        assert!(matches("boss", "office", &[], boss));
        assert!(matches("when the boss", "office", &[], boss));
        assert!(!matches("when the dog", "office", &[], boss));
        assert!(matches("office boss", "office", &[], boss));
        assert!(matches("^off boss", "office", &[], boss));
    }

    #[test]
    fn caption_ignores_case_and_accents() {
        assert!(matches("Boss", "office", &[], Some("WHEN THE BOSS WALKS IN")));
        assert!(matches("Cat", "cat jam", &[], Some("CAT")));
        assert!(matches("cafe", "morning", &[], Some("ÇA VA? CAFÉ TIME")));
        assert!(matches("café", "morning", &[], Some("ÇA VA? CAFÉ TIME")));
        assert!(matches("va?", "morning", &[], Some("ÇA VA? CAFÉ TIME")));
    }

    #[test]
    fn caption_fuzzy_is_a_word_prefix_never_a_subsequence() {
        let boss = Some("WHEN THE BOSS WALKS IN");
        assert!(matches("wal", "office", &[], boss));
        assert!(!matches("oss", "office", &[], boss));
        assert!(!matches("bss", "office", &[], boss));
        assert!(!matches("cj", "x", &[], Some("cat jam")));
        assert!(matches("cj", "cat jam", &[], Some("unrelated")));
        assert!(!matches("cat", "school", &[], Some("EDUCATION IS KEY")));
    }

    #[test]
    fn caption_substring_atoms_match_inside_words() {
        let boss = Some("WHEN THE BOSS WALKS IN");
        assert!(matches("'oss", "office", &[], boss));
        assert!(matches("'cat", "school", &[], Some("EDUCATION IS KEY")));
    }

    #[test]
    fn punctuation_breaks_words() {
        let xmen = Some("X-MEN ORIGINS");
        assert!(matches("men", "mutants", &[], xmen));
        assert!(matches("x-men", "mutants", &[], xmen));
        assert!(!matches("xmen", "mutants", &[], xmen));
    }

    #[test]
    fn anchors_ignore_the_caption() {
        let boss = Some("WHEN THE BOSS WALKS IN");
        assert!(!matches("^when", "office", &[], boss));
        assert!(!matches("in$", "office", &[], boss));
        assert!(!matches("^boss$", "office", &[], Some("boss")));
    }

    #[test]
    fn negation_also_excludes_caption_text() {
        let boss = Some("WHEN THE BOSS WALKS IN");
        assert!(!matches("!boss", "office", &[], boss));
        assert!(!matches("!BOSS", "office", &[], boss));
        assert!(matches("!dog", "office", &[], boss));
        assert!(matches("!boss", "office", &[], None));
        assert!(!matches("moon !wojak", "moon", &[], Some("the wojak moon")));
        assert!(matches("moon !wojak", "moon", &[], Some("the big moon")));
        assert!(matches("!^wojak", "moon", &[], Some("wojak")));
    }

    #[test]
    fn short_needles_skip_the_caption() {
        assert!(!matches("t", "x", &[], Some("don't")));
        assert!(!matches("'t", "x", &[], Some("tt")));
        assert!(matches("to", "x", &[], Some("tt to")));
    }

    #[test]
    fn unread_and_blank_captions_never_match() {
        assert!(!matches("landscape sky", "landscape", &[], Some("")));
        assert!(!matches("landscape sky", "landscape", &[], None));
        assert!(matches("landscape", "landscape", &[], None));
    }

    #[test]
    fn blank_and_unread_are_distinct_in_coverage() {
        let images = vec![
            image(1, "a", &[], None),
            image(2, "b", &[], Some("")),
            image(3, "c", &[], Some("text")),
            image(4, "d", &[], None),
        ];
        assert_eq!(run("", images).coverage, Coverage { images: 4, unread: 2 });
    }

    #[test]
    fn name_hits_outrank_caption_hits_whatever_the_library_order() {
        let got = order("boss", &[("office", Some("THE BOSS IS HERE")), ("boss baby", None)]);
        assert_eq!(got, ["boss baby", "office"]);
    }

    #[test]
    fn fewer_caption_atoms_rank_first() {
        let got =
            order("boss here", &[("office", Some("BOSS IS HERE")), ("boss", Some("HERE WE GO")), ("boss here", None)]);
        assert_eq!(got, ["boss here", "boss", "office"]);
    }

    #[test]
    fn caption_ties_keep_library_order() {
        assert_eq!(order("boss", &[("zeta", Some("BOSS TWO")), ("alpha", Some("BOSS ONE"))]), ["zeta", "alpha"]);
    }

    #[test]
    fn within_the_caption_tier_whole_words_beat_prefixes_beat_substrings() {
        let got =
            order("'moon", &[("a", Some("honeymoon")), ("b", Some("moonlight")), ("c", Some("full moon tonight"))]);
        assert_eq!(got, ["c", "b", "a"]);
    }

    #[test]
    fn atoms_split_across_fields() {
        let results = run("cat moon", vec![image(1, "cat jam", &[], Some("WHEN THE MOON HITS"))]);
        assert_eq!(names(&results), ["cat jam"]);
        let snippet = results.hits[0].snippet().expect("the caption took part");
        assert_eq!(
            snippet.segments.iter().filter(|s| s.matched).map(|s| s.text.as_str()).collect::<Vec<_>>(),
            ["MOON"]
        );
    }

    #[test]
    fn a_meta_match_has_no_snippet() {
        let results = run("moon", vec![image(1, "moon", &[], Some("the moon"))]);
        assert!(results.hits[0].snippet().is_none());
    }

    #[test]
    fn snippets_mark_matches_and_report_cuts() {
        let caption = format!("{} needle {}", "lorem ipsum ".repeat(12), "dolor sit ".repeat(12));
        let results = run("needle", vec![image(1, "x", &[], Some(&caption))]);
        let s = results.hits[0].snippet().unwrap();
        assert!(s.leading_cut && s.trailing_cut);
        let joined: String = s.segments.iter().map(|seg| seg.text.as_str()).collect();
        assert!(joined.len() <= SNIPPET_MAX + 10, "{joined:?}");
        assert!(CaptionText::normalize(&caption).text().contains(&joined), "contiguous slice of the caption");
        assert!(!joined.starts_with(' ') && !joined.ends_with(' '));
        let matched: Vec<_> = s.segments.iter().filter(|seg| seg.matched).map(|seg| seg.text.as_str()).collect();
        assert_eq!(matched, ["needle"]);

        let short = run("needle", vec![image(1, "x", &[], Some("a needle here"))]);
        let s = short.hits[0].snippet().unwrap();
        assert!(!s.leading_cut && !s.trailing_cut);
        assert_eq!(
            s.segments,
            [
                Segment { text: "a ".into(), matched: false },
                Segment { text: "needle".into(), matched: true },
                Segment { text: " here".into(), matched: false },
            ]
        );
    }

    #[test]
    fn snippets_handle_multibyte_text() {
        let caption = format!("{} ÇA VA? CAFÉ TIME {}", "éé ".repeat(30), "ñandú ".repeat(30));
        let results = run("café", vec![image(1, "x", &[], Some(&caption))]);
        let s = results.hits[0].snippet().unwrap();
        let matched: Vec<_> = s.segments.iter().filter(|seg| seg.matched).map(|seg| seg.text.as_str()).collect();
        assert_eq!(matched, ["CAFÉ"], "highlights the original text, not the folded one");
    }

    #[test]
    fn two_atoms_in_one_word_merge_into_one_segment() {
        let results = run("'moon 'moonlight", vec![image(1, "x", &[], Some("a moonlight b"))]);
        let s = results.hits[0].snippet().unwrap();
        let matched: Vec<_> = s.segments.iter().filter(|seg| seg.matched).map(|seg| seg.text.as_str()).collect();
        assert_eq!(matched, ["moonlight"]);
    }
}
