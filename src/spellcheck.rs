use std::{
    collections::{HashMap, HashSet},
    fs,
};

const SYSTEM_DICTIONARY_PATHS: [&str; 2] = ["/usr/share/dict/words", "/usr/dict/words"];

const BUILTIN_WORDS: &[&str] = &[
    "a", "about", "after", "again", "all", "also", "always", "an", "and", "another", "any", "are",
    "around", "as", "at", "back", "be", "because", "been", "before", "being", "between", "both",
    "but", "by", "can", "clear", "close", "day", "did", "do", "does", "doing", "done", "down",
    "draft", "edit", "entry", "error", "even", "every", "feel", "first", "flow", "for", "from",
    "full", "get", "good", "great", "had", "has", "have", "help", "here", "how", "i", "if", "in",
    "index", "into", "is", "it", "journal", "just", "keep", "key", "kind", "last", "later", "line",
    "little", "load", "look", "made", "make", "many", "menu", "mode", "more", "most", "move", "my",
    "need", "new", "next", "no", "not", "now", "of", "off", "old", "on", "one", "open", "or",
    "other", "our", "out", "over", "page", "path", "people", "quick", "ready", "really", "replace",
    "review", "right", "run", "same", "save", "saved", "search", "see", "session", "set",
    "settings", "should", "show", "simple", "small", "so", "some", "start", "status", "still",
    "sync", "take", "test", "text", "than", "that", "the", "their", "them", "then", "there",
    "these", "they", "thing", "this", "time", "to", "today", "too", "try", "up", "use", "user",
    "very", "view", "was", "we", "well", "were", "what", "when", "where", "which", "while", "will",
    "with", "word", "work", "write", "writer", "writing", "yes", "you", "your",
];

const COMMON_TYPOS: &[(&str, &str)] = &[
    ("teh", "the"),
    ("adn", "and"),
    ("becuase", "because"),
    ("definately", "definitely"),
    ("dont", "don't"),
    ("recieve", "receive"),
    ("seperate", "separate"),
    ("thier", "their"),
    ("wierd", "weird"),
    ("untill", "until"),
    ("occured", "occurred"),
    ("enviroment", "environment"),
    ("funciton", "function"),
    ("seach", "search"),
    ("jounral", "journal"),
    ("jounal", "journal"),
    ("wroking", "working"),
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TokenSpan {
    pub token: String,
    pub start_col: usize,
    pub end_col: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpellHit {
    pub row: usize,
    pub start_col: usize,
    pub end_col: usize,
    pub word: String,
    pub suggestions: Vec<String>,
}

pub struct SpellChecker {
    dictionary: HashSet<String>,
    by_initial: HashMap<char, Vec<String>>,
}

impl Default for SpellChecker {
    fn default() -> Self {
        Self::load()
    }
}

impl SpellChecker {
    pub fn load() -> Self {
        let mut dictionary = HashSet::new();
        for word in BUILTIN_WORDS {
            dictionary.insert((*word).to_string());
        }

        for path in SYSTEM_DICTIONARY_PATHS {
            if let Ok(contents) = fs::read_to_string(path) {
                for line in contents.lines() {
                    if let Some(normalized) = normalize_candidate_word(line) {
                        dictionary.insert(normalized);
                    }
                }
            }
        }

        let mut by_initial: HashMap<char, Vec<String>> = HashMap::new();
        for word in &dictionary {
            if let Some(initial) = word.chars().next() {
                by_initial.entry(initial).or_default().push(word.clone());
            }
        }
        for values in by_initial.values_mut() {
            values.sort_unstable();
        }

        Self {
            dictionary,
            by_initial,
        }
    }

    pub fn dictionary_size(&self) -> usize {
        self.dictionary.len()
    }

    pub fn check_lines<'a, I>(&self, lines: I, session_words: &HashSet<String>) -> Vec<SpellHit>
    where
        I: IntoIterator<Item = (usize, &'a str)>,
    {
        let mut hits = Vec::new();
        let mut suggestion_cache: HashMap<String, Vec<String>> = HashMap::new();
        for (row, line) in lines {
            for span in tokenize_line_with_positions(line) {
                if !should_check_word(&span.token) {
                    continue;
                }
                if self.is_word_known(&span.token, session_words) {
                    continue;
                }

                let normalized = normalize_word(&span.token);
                let suggestions = suggestion_cache
                    .entry(normalized)
                    .or_insert_with(|| self.suggest(&span.token, 3))
                    .clone();
                hits.push(SpellHit {
                    row,
                    start_col: span.start_col,
                    end_col: span.end_col,
                    word: span.token,
                    suggestions,
                });
            }
        }
        hits
    }

    pub fn check_text(&self, text: &str, session_words: &HashSet<String>) -> Vec<SpellHit> {
        self.check_lines(text.lines().enumerate(), session_words)
    }

    pub fn suggest(&self, word: &str, limit: usize) -> Vec<String> {
        if limit == 0 {
            return Vec::new();
        }
        if let Some(correction) = Self::common_typo_correction(word) {
            return vec![correction.to_string()];
        }

        let normalized = normalize_word(word);
        let Some(initial) = normalized.chars().next() else {
            return Vec::new();
        };
        let max_distance = match normalized.chars().count() {
            0..=4 => 1usize,
            5..=8 => 2usize,
            _ => 3usize,
        };

        let mut scored = Vec::<(usize, usize, String)>::new();
        if let Some(candidates) = self.by_initial.get(&initial) {
            for candidate in candidates {
                let len_diff = normalized
                    .chars()
                    .count()
                    .abs_diff(candidate.chars().count());
                if len_diff > 3 {
                    continue;
                }
                let distance = damerau_levenshtein(&normalized, candidate);
                if distance <= max_distance {
                    scored.push((distance, len_diff, candidate.clone()));
                }
            }
        }
        scored.sort_by(|left, right| left.cmp(right).then_with(|| left.2.cmp(&right.2)));
        scored.dedup_by(|left, right| left.2 == right.2);
        scored.into_iter().take(limit).map(|(_, _, c)| c).collect()
    }

    pub fn is_word_known(&self, word: &str, session_words: &HashSet<String>) -> bool {
        let normalized = normalize_word(word);
        if normalized.is_empty() {
            return true;
        }
        if session_words.contains(&normalized) || self.dictionary.contains(&normalized) {
            return true;
        }
        stem_candidates(&normalized)
            .into_iter()
            .any(|stem| session_words.contains(&stem) || self.dictionary.contains(&stem))
    }

    pub fn common_typo_correction(word: &str) -> Option<&'static str> {
        let normalized = normalize_word(word);
        COMMON_TYPOS
            .iter()
            .find_map(|(wrong, correct)| (*wrong == normalized).then_some(*correct))
    }
}

pub fn tokenize_line_with_positions(line: &str) -> Vec<TokenSpan> {
    let mut spans = Vec::new();
    let mut current = String::new();
    let mut start_col = 0usize;
    let mut col = 0usize;

    for ch in line.chars() {
        if is_token_char(ch) {
            if current.is_empty() {
                start_col = col;
            }
            current.push(ch);
        } else if !current.is_empty() {
            push_trimmed_token(&mut spans, std::mem::take(&mut current), start_col, col);
        }
        col += 1;
    }
    if !current.is_empty() {
        push_trimmed_token(&mut spans, current, start_col, col);
    }
    spans
}

pub fn session_word(word: &str) -> Option<String> {
    let normalized = normalize_word(word);
    (!normalized.is_empty()).then_some(normalized)
}

fn push_trimmed_token(out: &mut Vec<TokenSpan>, token: String, start_col: usize, end_col: usize) {
    let trimmed_left = token.chars().take_while(|ch| *ch == '\'').count();
    let trimmed_right = token.chars().rev().take_while(|ch| *ch == '\'').count();
    let total = token.chars().count();
    if trimmed_left + trimmed_right >= total {
        return;
    }
    let keep = total - trimmed_left - trimmed_right;
    let trimmed = token
        .chars()
        .skip(trimmed_left)
        .take(keep)
        .collect::<String>();
    out.push(TokenSpan {
        token: trimmed,
        start_col: start_col + trimmed_left,
        end_col: end_col.saturating_sub(trimmed_right),
    });
}

fn normalize_candidate_word(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.len() > 32 || trimmed.is_empty() {
        return None;
    }
    if !trimmed.chars().all(|ch| ch.is_alphabetic() || ch == '\'') {
        return None;
    }
    let normalized = normalize_word(trimmed);
    (normalized.chars().count() >= 2).then_some(normalized)
}

fn normalize_word(input: &str) -> String {
    input.trim_matches('\'').to_lowercase()
}

fn stem_candidates(normalized: &str) -> Vec<String> {
    let mut stems = Vec::new();
    if let Some(base) = normalized.strip_suffix("'s")
        && base.len() >= 2
    {
        stems.push(base.to_string());
    }
    if let Some(base) = normalized.strip_suffix("ies")
        && base.len() >= 2
    {
        stems.push(format!("{base}y"));
    }
    if let Some(base) = normalized.strip_suffix("es")
        && base.len() >= 3
    {
        stems.push(base.to_string());
    }
    if let Some(base) = normalized.strip_suffix('s')
        && base.len() >= 3
    {
        stems.push(base.to_string());
    }
    if let Some(base) = normalized.strip_suffix("ing")
        && base.len() >= 3
    {
        stems.push(base.to_string());
        stems.push(format!("{base}e"));
    }
    if let Some(base) = normalized.strip_suffix("ed")
        && base.len() >= 3
    {
        stems.push(base.to_string());
        stems.push(format!("{base}e"));
    }
    stems
}

fn should_check_word(word: &str) -> bool {
    let chars = word.chars().collect::<Vec<_>>();
    if chars.len() <= 1 {
        return false;
    }
    if chars.iter().all(|ch| ch.is_ascii_uppercase()) && chars.len() <= 5 {
        return false;
    }
    chars.iter().any(|ch| ch.is_alphabetic())
}

fn is_token_char(ch: char) -> bool {
    ch.is_alphabetic() || ch == '\''
}

fn damerau_levenshtein(left: &str, right: &str) -> usize {
    if left == right {
        return 0;
    }
    if left.is_empty() {
        return right.chars().count();
    }
    if right.is_empty() {
        return left.chars().count();
    }

    let left_chars = left.chars().collect::<Vec<_>>();
    let right_chars = right.chars().collect::<Vec<_>>();
    let mut costs = vec![0usize; right_chars.len() + 1];
    for (idx, cost) in costs.iter_mut().enumerate() {
        *cost = idx;
    }

    for (i, left_ch) in left_chars.iter().enumerate() {
        let mut last_diagonal = i;
        let mut current = i + 1;
        for (j, right_ch) in right_chars.iter().enumerate() {
            let insert_cost = costs[j + 1] + 1;
            let delete_cost = current + 1;
            let replace_cost = last_diagonal + usize::from(left_ch != right_ch);
            last_diagonal = costs[j + 1];
            current = insert_cost.min(delete_cost).min(replace_cost);

            if i > 0 && j > 0 && *left_ch == right_chars[j - 1] && left_chars[i - 1] == *right_ch {
                current = current.min(costs[j - 1] + 1);
            }
            costs[j + 1] = current;
        }
        costs[0] = i + 1;
    }
    costs[right_chars.len()]
}

#[cfg(test)]
mod tests {
    use super::{SpellChecker, session_word, tokenize_line_with_positions};
    use std::collections::HashSet;

    #[test]
    fn tokenization_tracks_positions_and_apostrophes() {
        let spans = tokenize_line_with_positions("it's  teh  'journal'");
        assert_eq!(spans[0].token, "it's");
        assert_eq!(spans[0].start_col, 0);
        assert_eq!(spans[0].end_col, 4);
        assert_eq!(spans[1].token, "teh");
        assert_eq!(spans[1].start_col, 6);
        assert_eq!(spans[2].token, "journal");
    }

    #[test]
    fn spellcheck_flags_common_typo_and_suggests_fix() {
        let checker = SpellChecker::load();
        let hits = checker.check_text("teh journal entry", &HashSet::new());
        assert!(hits.iter().any(|hit| {
            hit.word == "teh" && hit.suggestions.first().map(String::as_str) == Some("the")
        }));
    }

    #[test]
    fn spellcheck_respects_session_dictionary() {
        let checker = SpellChecker::load();
        let mut session = HashSet::new();
        session.insert("awassee".to_string());
        let hits = checker.check_text("Awassee shipped this", &session);
        assert!(
            !hits
                .iter()
                .any(|hit| hit.word.eq_ignore_ascii_case("awassee"))
        );
    }

    #[test]
    fn acronym_is_not_flagged() {
        let checker = SpellChecker::load();
        let hits = checker.check_text("TUI UX API", &HashSet::new());
        assert!(hits.is_empty());
    }

    #[test]
    fn suggestion_prefers_nearest_word() {
        let checker = SpellChecker::load();
        let suggestions = checker.suggest("jounral", 3);
        assert!(suggestions.iter().any(|item| item == "journal"));
    }

    #[test]
    fn stemming_accepts_plural_word_forms() {
        let checker = SpellChecker::load();
        let hits = checker.check_text("entries", &HashSet::new());
        assert!(hits.is_empty());
    }

    #[test]
    fn session_word_normalizes_case() {
        assert_eq!(session_word("Journal"), Some("journal".to_string()));
        assert_eq!(session_word("''"), None);
    }

    #[test]
    fn dictionary_loads_words() {
        let checker = SpellChecker::load();
        assert!(checker.dictionary_size() > 20);
    }
}
