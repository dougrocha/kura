use std::cmp::Reverse;

use nucleo_matcher::{
    Matcher, Utf32Str,
    pattern::{CaseMatching, Normalization, Pattern},
};

use crate::images::ImageWithTags;

/// A parsed search query. Words starting with `#` keep only images that have a
/// tag starting with that word. Everything else is fuzzy matched against the
/// image name and its tags.
pub struct Query {
    tags: Vec<String>,
    pattern: Pattern,
}

impl Query {
    pub fn parse(input: &str) -> Self {
        let mut tags = vec![];
        let mut words = vec![];
        for word in input.split_whitespace() {
            match word.strip_prefix('#') {
                Some("") => {}
                Some(tag) => tags.push(tag.to_lowercase()),
                None => words.push(word),
            }
        }

        let pattern = Pattern::parse(&words.join(" "), CaseMatching::Smart, Normalization::Smart);
        Self { tags, pattern }
    }

    pub fn is_empty(&self) -> bool {
        self.tags.is_empty() && self.pattern.atoms.is_empty()
    }

    fn score(
        &self,
        name: &str,
        tags: &[&str],
        matcher: &mut Matcher,
        buf: &mut Vec<char>,
    ) -> Option<u32> {
        let has_tags = self.tags.iter().all(|wanted| {
            tags.iter()
                .any(|tag| tag.to_lowercase().starts_with(wanted.as_str()))
        });
        if !has_tags {
            return None;
        }

        if self.pattern.atoms.is_empty() {
            return Some(0);
        }

        let haystack = std::iter::once(name)
            .chain(tags.iter().copied())
            .collect::<Vec<_>>()
            .join(" ");
        self.pattern.score(Utf32Str::new(&haystack, buf), matcher)
    }
}

/// Returns the indices of the images that match `query`, best match first.
/// An empty query keeps every image in its original order.
pub fn filter(images: &[ImageWithTags], query: &str, matcher: &mut Matcher) -> Vec<usize> {
    let query = Query::parse(query);
    if query.is_empty() {
        return (0..images.len()).collect();
    }

    let mut buf = vec![];
    let mut scored: Vec<(usize, u32)> = images
        .iter()
        .enumerate()
        .filter_map(|(i, img)| {
            let tags: Vec<&str> = img.tags.iter().map(|t| t.tag.as_str()).collect();
            query
                .score(&img.image.name, &tags, matcher, &mut buf)
                .map(|score| (i, score))
        })
        .collect();

    // Stable sort, so equal scores keep the original order.
    scored.sort_by_key(|&(_, score)| Reverse(score));
    scored.into_iter().map(|(i, _)| i).collect()
}

#[cfg(test)]
mod tests {
    use nucleo_matcher::Config;

    use super::*;

    fn matches(query: &str, name: &str, tags: &[&str]) -> bool {
        let mut matcher = Matcher::new(Config::DEFAULT);
        Query::parse(query)
            .score(name, tags, &mut matcher, &mut vec![])
            .is_some()
    }

    #[test]
    fn empty_query_matches_everything() {
        assert!(Query::parse("").is_empty());
        assert!(Query::parse("  # ").is_empty());
        assert!(matches("", "anything", &[]));
    }

    #[test]
    fn fuzzy_matches_name() {
        assert!(matches("cat", "cat vibing", &[]));
        assert!(matches("cvb", "cat vibing", &[]));
        assert!(!matches("dog", "cat vibing", &[]));
    }

    #[test]
    fn fuzzy_matches_tags() {
        assert!(matches("reaction", "cat vibing", &["reaction"]));
    }

    #[test]
    fn hash_words_filter_by_tag_prefix() {
        assert!(matches("#reac", "cat vibing", &["Reaction"]));
        assert!(!matches("#reac", "cat vibing", &["music"]));
        assert!(matches("#reac #mus", "cat vibing", &["reaction", "music"]));
        assert!(!matches("#reac #mus", "cat vibing", &["reaction"]));
    }

    #[test]
    fn tag_filter_and_fuzzy_combine() {
        assert!(matches("#reaction cat", "cat vibing", &["reaction"]));
        assert!(!matches("#reaction dog", "cat vibing", &["reaction"]));
    }
}
