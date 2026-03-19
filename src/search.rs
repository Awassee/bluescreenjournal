use chrono::NaiveDate;
use std::collections::{HashMap, HashSet};
use zeroize::Zeroize;

const SNIPPET_CONTEXT_CHARS: usize = 24;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchDocument {
    pub date: NaiveDate,
    pub entry_number: String,
    pub body: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchQuery {
    pub text: String,
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SearchOptions {
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub context_chars: usize,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            case_sensitive: false,
            whole_word: false,
            context_chars: SNIPPET_CONTEXT_CHARS,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snippet {
    pub text: String,
    pub highlight_start: usize,
    pub highlight_end: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchResult {
    pub date: NaiveDate,
    pub entry_number: String,
    pub snippet: Snippet,
    pub row: usize,
    pub start_col: usize,
    pub end_col: usize,
    pub matched_text: String,
}

struct IndexedDocument {
    date: NaiveDate,
    entry_number: String,
    body: String,
    body_lower: String,
}

pub struct SearchIndex {
    documents: Vec<IndexedDocument>,
    postings: HashMap<String, Vec<usize>>,
}

impl SearchIndex {
    pub fn build(mut documents: Vec<SearchDocument>) -> Self {
        documents.sort_unstable_by(|left, right| right.date.cmp(&left.date));

        let mut indexed_documents = Vec::with_capacity(documents.len());
        let mut postings = HashMap::<String, Vec<usize>>::new();

        for document in documents {
            let doc_id = indexed_documents.len();
            let mut unique_tokens = HashSet::new();
            for token in tokenize(&document.body) {
                if unique_tokens.insert(token.clone()) {
                    postings.entry(token).or_default().push(doc_id);
                }
            }
            indexed_documents.push(IndexedDocument {
                date: document.date,
                entry_number: document.entry_number,
                body_lower: document.body.to_lowercase(),
                body: document.body,
            });
        }

        Self {
            documents: indexed_documents,
            postings,
        }
    }

    pub fn search(&self, query: &SearchQuery) -> Vec<SearchResult> {
        self.search_with_options(query, &SearchOptions::default())
    }

    pub fn search_with_options(
        &self,
        query: &SearchQuery,
        options: &SearchOptions,
    ) -> Vec<SearchResult> {
        let query_text = query.text.trim();
        if query_text.is_empty() {
            return Vec::new();
        }

        let query_tokens = tokenize_with_mode(query_text, options.case_sensitive);
        let lowered_query = query_text.to_lowercase();
        let candidate_ids = if options.case_sensitive {
            None
        } else {
            self.candidate_documents(&query_tokens)
        };

        let mut results = Vec::new();
        for (doc_id, document) in self.documents.iter().enumerate() {
            if !matches_date_filter(document.date, query.from, query.to) {
                continue;
            }
            if let Some(candidate_ids) = &candidate_ids
                && !candidate_ids.contains(&doc_id)
            {
                continue;
            }

            let Some(raw_match) = locate_match_with_options(
                document,
                query_text,
                &lowered_query,
                &query_tokens,
                options,
            ) else {
                continue;
            };

            let (row, start_col, end_col) = byte_range_to_match_position(
                &document.body,
                raw_match.start_byte,
                raw_match.end_byte,
            );
            let line =
                line_for_byte_range(&document.body, raw_match.start_byte, raw_match.end_byte);
            let snippet = generate_snippet(&line, start_col, end_col, options.context_chars);
            let matched_text = document
                .body
                .get(raw_match.start_byte..raw_match.end_byte)
                .unwrap_or_default()
                .to_string();

            results.push(SearchResult {
                date: document.date,
                entry_number: document.entry_number.clone(),
                snippet,
                row,
                start_col,
                end_col,
                matched_text,
            });
        }

        results
    }

    fn candidate_documents(&self, query_tokens: &[String]) -> Option<HashSet<usize>> {
        if query_tokens.is_empty() {
            return None;
        }

        let mut postings = Vec::with_capacity(query_tokens.len());
        for token in query_tokens {
            let Some(ids) = self.postings.get(token) else {
                return Some(HashSet::new());
            };
            postings.push(ids);
        }
        postings.sort_by_key(|ids| ids.len());

        let mut candidate_ids = postings[0].iter().copied().collect::<HashSet<_>>();
        for ids in postings.iter().skip(1) {
            candidate_ids.retain(|doc_id| ids.binary_search(doc_id).is_ok());
        }
        Some(candidate_ids)
    }

    pub fn wipe(&mut self) {
        for document in &mut self.documents {
            document.entry_number.zeroize();
            document.body.zeroize();
            document.body_lower.zeroize();
        }
        self.documents.clear();

        for (mut token, mut doc_ids) in std::mem::take(&mut self.postings) {
            token.zeroize();
            doc_ids.clear();
        }
    }
}

pub fn tokenize(input: &str) -> Vec<String> {
    tokenize_with_mode(input, false)
}

fn tokenize_with_mode(input: &str, case_sensitive: bool) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();

    for ch in input.chars() {
        if ch.is_alphanumeric() {
            if case_sensitive {
                current.push(ch);
            } else {
                current.extend(ch.to_lowercase());
            }
        } else if !current.is_empty() {
            tokens.push(std::mem::take(&mut current));
        }
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    tokens
}

pub fn matches_date_filter(
    date: NaiveDate,
    from: Option<NaiveDate>,
    to: Option<NaiveDate>,
) -> bool {
    if let Some(from) = from
        && date < from
    {
        return false;
    }
    if let Some(to) = to
        && date > to
    {
        return false;
    }
    true
}

pub fn generate_snippet(
    line: &str,
    match_start_col: usize,
    match_end_col: usize,
    context_chars: usize,
) -> Snippet {
    let line_chars = line.chars().count();
    let snippet_start = match_start_col.saturating_sub(context_chars);
    let snippet_end = (match_end_col + context_chars).min(line_chars);

    let mut text = slice_chars(line, snippet_start, snippet_end);
    let mut highlight_start = match_start_col.saturating_sub(snippet_start);
    let mut highlight_end = match_end_col.saturating_sub(snippet_start);

    if snippet_start > 0 {
        text = format!("...{text}");
        highlight_start += 3;
        highlight_end += 3;
    }
    if snippet_end < line_chars {
        text.push_str("...");
    }

    Snippet {
        text,
        highlight_start,
        highlight_end,
    }
}

pub fn format_cli_snippet(snippet: &Snippet) -> String {
    let before = slice_chars(&snippet.text, 0, snippet.highlight_start);
    let highlight = slice_chars(
        &snippet.text,
        snippet.highlight_start,
        snippet.highlight_end,
    );
    let after = slice_chars(
        &snippet.text,
        snippet.highlight_end,
        snippet.text.chars().count(),
    );
    format!("{before}[{highlight}]{after}")
}

fn locate_match_with_options(
    document: &IndexedDocument,
    query_text: &str,
    lowered_query: &str,
    query_tokens: &[String],
    options: &SearchOptions,
) -> Option<RawMatch> {
    if options.case_sensitive {
        if let Some((start_byte, end_byte)) =
            find_match(&document.body, query_text, options.whole_word)
        {
            return Some(RawMatch {
                start_byte,
                end_byte,
            });
        }
    } else if let Some((start_byte, end_byte)) =
        find_match(&document.body_lower, lowered_query, options.whole_word)
        && document.body.get(start_byte..end_byte).is_some()
    {
        return Some(RawMatch {
            start_byte,
            end_byte,
        });
    }

    for token in query_tokens {
        let candidate = if options.case_sensitive {
            find_match(&document.body, token, options.whole_word)
        } else {
            find_match(&document.body_lower, token, options.whole_word)
        };
        if let Some((start_byte, end_byte)) = candidate
            && document.body.get(start_byte..end_byte).is_some()
            && (!options.whole_word || is_word_boundary_match(&document.body, start_byte, end_byte))
        {
            return Some(RawMatch {
                start_byte,
                end_byte,
            });
        }
    }

    None
}

fn find_match(haystack: &str, needle: &str, whole_word: bool) -> Option<(usize, usize)> {
    if needle.is_empty() {
        return None;
    }

    let mut search_start = 0usize;
    while let Some(relative) = haystack[search_start..].find(needle) {
        let start = search_start + relative;
        let end = start + needle.len();
        if !whole_word || is_word_boundary_match(haystack, start, end) {
            return Some((start, end));
        }
        if end >= haystack.len() {
            break;
        }
        search_start = end;
    }

    None
}

fn is_word_boundary_match(haystack: &str, start_byte: usize, end_byte: usize) -> bool {
    let left = haystack[..start_byte].chars().next_back();
    let right = haystack[end_byte..].chars().next();
    !left.is_some_and(is_word_char) && !right.is_some_and(is_word_char)
}

fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

fn byte_range_to_match_position(
    body: &str,
    start_byte: usize,
    end_byte: usize,
) -> (usize, usize, usize) {
    let line_start = body[..start_byte].rfind('\n').map_or(0, |idx| idx + 1);
    let row = body[..start_byte].chars().filter(|ch| *ch == '\n').count();
    let start_col = body[line_start..start_byte].chars().count();
    let line_end = body[end_byte..]
        .find('\n')
        .map_or(body.len(), |offset| end_byte + offset);
    let clamped_end = end_byte.min(line_end);
    let end_col = start_col + body[start_byte..clamped_end].chars().count();
    (row, start_col, end_col)
}

fn line_for_byte_range(body: &str, start_byte: usize, end_byte: usize) -> String {
    let line_start = body[..start_byte].rfind('\n').map_or(0, |idx| idx + 1);
    let line_end = body[end_byte..]
        .find('\n')
        .map_or(body.len(), |offset| end_byte + offset);
    body[line_start..line_end].replace('\t', " ")
}

fn slice_chars(input: &str, start: usize, end: usize) -> String {
    input
        .chars()
        .skip(start)
        .take(end.saturating_sub(start))
        .collect()
}

struct RawMatch {
    start_byte: usize,
    end_byte: usize,
}

#[cfg(test)]
mod tests {
    use super::{
        SearchDocument, SearchIndex, SearchOptions, SearchQuery, format_cli_snippet,
        generate_snippet, matches_date_filter, tokenize,
    };
    use chrono::NaiveDate;

    #[test]
    fn tokenizer_splits_and_normalizes_words() {
        assert_eq!(
            tokenize("Work log, 2026-03-16!"),
            vec!["work", "log", "2026", "03", "16"]
        );
    }

    #[test]
    fn snippet_generation_adds_context_and_highlight_range() {
        let snippet = generate_snippet("abcdefg hijklmnop qrstuv", 8, 13, 4);
        assert_eq!(snippet.text, "...efg hijklmnop...");
        assert_eq!(snippet.highlight_start, 7);
        assert_eq!(snippet.highlight_end, 12);
    }

    #[test]
    fn date_filter_honors_from_and_to_bounds() {
        let date = NaiveDate::from_ymd_opt(2026, 3, 16).expect("date");
        assert!(matches_date_filter(
            date,
            Some(NaiveDate::from_ymd_opt(2026, 3, 1).expect("date")),
            Some(NaiveDate::from_ymd_opt(2026, 3, 31).expect("date"))
        ));
        assert!(!matches_date_filter(
            date,
            Some(NaiveDate::from_ymd_opt(2026, 3, 17).expect("date")),
            None
        ));
        assert!(!matches_date_filter(
            date,
            None,
            Some(NaiveDate::from_ymd_opt(2026, 3, 15).expect("date"))
        ));
    }

    #[test]
    fn search_index_returns_filtered_results() {
        let documents = vec![
            SearchDocument {
                date: NaiveDate::from_ymd_opt(2026, 3, 16).expect("date"),
                entry_number: "0000016".to_string(),
                body: "Blue screen work note".to_string(),
            },
            SearchDocument {
                date: NaiveDate::from_ymd_opt(2026, 3, 12).expect("date"),
                entry_number: "0000012".to_string(),
                body: "Weekend walk note".to_string(),
            },
        ];
        let index = SearchIndex::build(documents);

        let results = index.search(&SearchQuery {
            text: "note".to_string(),
            from: Some(NaiveDate::from_ymd_opt(2026, 3, 15).expect("date")),
            to: None,
        });

        assert_eq!(results.len(), 1);
        assert_eq!(
            results[0].date,
            NaiveDate::from_ymd_opt(2026, 3, 16).expect("date")
        );
    }

    #[test]
    fn cli_snippet_marks_highlight_with_brackets() {
        let snippet = generate_snippet("Blue screen work note", 5, 11, 5);
        assert_eq!(format_cli_snippet(&snippet), "Blue [screen] work...");
    }

    #[test]
    fn case_sensitive_search_only_matches_exact_case() {
        let index = SearchIndex::build(vec![SearchDocument {
            date: NaiveDate::from_ymd_opt(2026, 3, 19).expect("date"),
            entry_number: "0000001".to_string(),
            body: "Work started.\nwork paused.".to_string(),
        }]);

        let strict = index.search_with_options(
            &SearchQuery {
                text: "Work".to_string(),
                from: None,
                to: None,
            },
            &SearchOptions {
                case_sensitive: true,
                ..SearchOptions::default()
            },
        );
        assert_eq!(strict.len(), 1);
        assert_eq!(strict[0].matched_text, "Work");
    }

    #[test]
    fn whole_word_search_ignores_partial_matches() {
        let index = SearchIndex::build(vec![SearchDocument {
            date: NaiveDate::from_ymd_opt(2026, 3, 19).expect("date"),
            entry_number: "0000001".to_string(),
            body: "noteworthy note".to_string(),
        }]);

        let options = SearchOptions {
            whole_word: true,
            ..SearchOptions::default()
        };
        let results = index.search_with_options(
            &SearchQuery {
                text: "note".to_string(),
                from: None,
                to: None,
            },
            &options,
        );
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].matched_text, "note");
    }

    #[test]
    fn snippet_context_size_follows_options() {
        let index = SearchIndex::build(vec![SearchDocument {
            date: NaiveDate::from_ymd_opt(2026, 3, 19).expect("date"),
            entry_number: "0000001".to_string(),
            body: "alpha bravo charlie delta echo".to_string(),
        }]);

        let short = index.search_with_options(
            &SearchQuery {
                text: "charlie".to_string(),
                from: None,
                to: None,
            },
            &SearchOptions {
                context_chars: 2,
                ..SearchOptions::default()
            },
        );
        assert_eq!(short.len(), 1);
        assert_eq!(short[0].snippet.text, "...o charlie d...");
    }
}
