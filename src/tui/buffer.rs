use zeroize::Zeroize;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatchPos {
    pub row: usize,
    pub start_col: usize,
    pub end_col: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BufferStats {
    pub lines: usize,
    pub words: usize,
    pub chars: usize,
}

#[derive(Clone, Debug)]
pub struct TextBuffer {
    lines: Vec<String>,
    cursor_row: usize,
    cursor_col: usize,
}

impl TextBuffer {
    pub fn new() -> Self {
        Self {
            lines: vec![String::new()],
            cursor_row: 0,
            cursor_col: 0,
        }
    }

    pub fn from_text(input: &str) -> Self {
        let mut lines = input
            .split('\n')
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        if lines.is_empty() {
            lines.push(String::new());
        }
        Self {
            lines,
            cursor_row: 0,
            cursor_col: 0,
        }
    }

    pub fn to_text(&self) -> String {
        self.lines.join("\n")
    }

    pub fn stats(&self) -> BufferStats {
        let lines = self.lines.len();
        let mut words = 0usize;
        let mut chars = lines.saturating_sub(1);
        for line in &self.lines {
            words += line.split_whitespace().count();
            chars += line.chars().count();
        }
        BufferStats {
            lines,
            words,
            chars,
        }
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    pub fn line(&self, row: usize) -> Option<&str> {
        self.lines.get(row).map(String::as_str)
    }

    pub fn cursor(&self) -> (usize, usize) {
        (self.cursor_row, self.cursor_col)
    }

    pub fn cursor_row(&self) -> usize {
        self.cursor_row
    }

    pub fn set_cursor(&mut self, row: usize, col: usize) {
        self.cursor_row = row.min(self.lines.len().saturating_sub(1));
        self.cursor_col = col.min(line_len_chars(&self.lines[self.cursor_row]));
    }

    pub fn insert_char(&mut self, ch: char) {
        let line = &mut self.lines[self.cursor_row];
        let idx = char_to_byte_idx(line, self.cursor_col);
        line.insert(idx, ch);
        self.cursor_col += 1;
    }

    pub fn insert_text(&mut self, text: &str) {
        for ch in text.chars() {
            if ch == '\n' {
                self.insert_newline();
            } else {
                self.insert_char(ch);
            }
        }
    }

    pub fn wrap_current_line(&mut self, max_cols: usize) {
        if max_cols == 0 || self.cursor_row >= self.lines.len() {
            return;
        }

        let row = self.cursor_row;
        let original_line = self.lines.remove(row);
        let line_len = line_len_chars(&original_line);
        if line_len <= max_cols {
            self.lines.insert(row, original_line);
            return;
        }

        let cursor_col = self.cursor_col.min(line_len);
        let wrapped = wrap_line_to_width(original_line, max_cols);
        let mut remaining_col = cursor_col;
        let mut new_cursor_row = row;
        let mut new_cursor_col = 0usize;

        for (idx, segment) in wrapped.iter().enumerate() {
            let segment_len = line_len_chars(segment);
            if remaining_col <= segment_len {
                new_cursor_row = row + idx;
                new_cursor_col = remaining_col;
                break;
            }
            remaining_col -= segment_len;
        }

        for (offset, segment) in wrapped.into_iter().enumerate() {
            self.lines.insert(row + offset, segment);
        }

        self.cursor_row = new_cursor_row;
        self.cursor_col = new_cursor_col;
    }

    pub fn insert_newline(&mut self) {
        let line = &mut self.lines[self.cursor_row];
        let idx = char_to_byte_idx(line, self.cursor_col);
        let right = line.split_off(idx);
        self.cursor_row += 1;
        self.cursor_col = 0;
        self.lines.insert(self.cursor_row, right);
    }

    pub fn backspace(&mut self) {
        if self.cursor_col > 0 {
            let line = &mut self.lines[self.cursor_row];
            let end = char_to_byte_idx(line, self.cursor_col);
            let start = char_to_byte_idx(line, self.cursor_col - 1);
            line.drain(start..end);
            self.cursor_col -= 1;
            return;
        }
        if self.cursor_row == 0 {
            return;
        }
        let current = self.lines.remove(self.cursor_row);
        self.cursor_row -= 1;
        let prev = &mut self.lines[self.cursor_row];
        let prev_len = line_len_chars(prev);
        prev.push_str(&current);
        self.cursor_col = prev_len;
    }

    pub fn delete(&mut self) {
        let row = self.cursor_row;
        let col = self.cursor_col;
        let line_len = line_len_chars(&self.lines[row]);
        if col < line_len {
            let line = &mut self.lines[row];
            let start = char_to_byte_idx(line, col);
            let end = char_to_byte_idx(line, col + 1);
            line.drain(start..end);
            return;
        }
        if row + 1 < self.lines.len() {
            let next = self.lines.remove(row + 1);
            self.lines[row].push_str(&next);
        }
    }

    pub fn move_left(&mut self) {
        if self.cursor_col > 0 {
            self.cursor_col -= 1;
            return;
        }
        if self.cursor_row > 0 {
            self.cursor_row -= 1;
            self.cursor_col = line_len_chars(&self.lines[self.cursor_row]);
        }
    }

    pub fn move_right(&mut self) {
        let line_len = line_len_chars(&self.lines[self.cursor_row]);
        if self.cursor_col < line_len {
            self.cursor_col += 1;
            return;
        }
        if self.cursor_row + 1 < self.lines.len() {
            self.cursor_row += 1;
            self.cursor_col = 0;
        }
    }

    pub fn move_up(&mut self) {
        if self.cursor_row == 0 {
            return;
        }
        self.cursor_row -= 1;
        self.cursor_col = self
            .cursor_col
            .min(line_len_chars(&self.lines[self.cursor_row]));
    }

    pub fn move_down(&mut self) {
        if self.cursor_row + 1 >= self.lines.len() {
            return;
        }
        self.cursor_row += 1;
        self.cursor_col = self
            .cursor_col
            .min(line_len_chars(&self.lines[self.cursor_row]));
    }

    pub fn move_paragraph_up(&mut self) {
        if self.cursor_row == 0 {
            return;
        }

        let mut row = self.cursor_row - 1;
        while row > 0 && self.lines[row].trim().is_empty() {
            row -= 1;
        }
        while row > 0 && !self.lines[row - 1].trim().is_empty() {
            row -= 1;
        }

        self.cursor_row = row;
        self.cursor_col = self
            .cursor_col
            .min(line_len_chars(&self.lines[self.cursor_row]));
    }

    pub fn move_paragraph_down(&mut self) {
        if self.cursor_row + 1 >= self.lines.len() {
            return;
        }

        let mut row = self.cursor_row;
        if !self.lines[row].trim().is_empty() {
            while row + 1 < self.lines.len() && !self.lines[row + 1].trim().is_empty() {
                row += 1;
            }
        }
        while row + 1 < self.lines.len() && self.lines[row + 1].trim().is_empty() {
            row += 1;
        }
        if row + 1 < self.lines.len() {
            row += 1;
        }

        self.cursor_row = row;
        self.cursor_col = self
            .cursor_col
            .min(line_len_chars(&self.lines[self.cursor_row]));
    }

    pub fn duplicate_current_line(&mut self) {
        let line = self.lines[self.cursor_row].clone();
        self.lines.insert(self.cursor_row + 1, line);
        self.cursor_row += 1;
        self.cursor_col = self
            .cursor_col
            .min(line_len_chars(&self.lines[self.cursor_row]));
    }

    pub fn delete_current_line(&mut self) {
        if self.lines.len() == 1 {
            self.lines[0].zeroize();
            self.lines[0].clear();
            self.cursor_row = 0;
            self.cursor_col = 0;
            return;
        }

        let removed = self.lines.remove(self.cursor_row);
        let mut removed = removed;
        removed.zeroize();
        if self.cursor_row >= self.lines.len() {
            self.cursor_row = self.lines.len() - 1;
        }
        self.cursor_col = self
            .cursor_col
            .min(line_len_chars(&self.lines[self.cursor_row]));
    }

    pub fn move_line_up(&mut self) {
        if self.cursor_row == 0 {
            return;
        }
        self.lines.swap(self.cursor_row, self.cursor_row - 1);
        self.cursor_row -= 1;
        self.cursor_col = self
            .cursor_col
            .min(line_len_chars(&self.lines[self.cursor_row]));
    }

    pub fn move_line_down(&mut self) {
        if self.cursor_row + 1 >= self.lines.len() {
            return;
        }
        self.lines.swap(self.cursor_row, self.cursor_row + 1);
        self.cursor_row += 1;
        self.cursor_col = self
            .cursor_col
            .min(line_len_chars(&self.lines[self.cursor_row]));
    }

    pub fn insert_blank_line_above(&mut self) {
        self.lines.insert(self.cursor_row, String::new());
        self.cursor_col = 0;
    }

    pub fn insert_blank_line_below(&mut self) {
        self.cursor_row += 1;
        self.lines.insert(self.cursor_row, String::new());
        self.cursor_col = 0;
    }

    pub fn move_to_line_start(&mut self) {
        self.cursor_col = 0;
    }

    pub fn move_to_line_end(&mut self) {
        self.cursor_col = line_len_chars(&self.lines[self.cursor_row]);
    }

    pub fn move_to_top(&mut self) {
        self.cursor_row = 0;
        self.cursor_col = self
            .cursor_col
            .min(line_len_chars(&self.lines[self.cursor_row]));
    }

    pub fn move_to_bottom(&mut self) {
        self.cursor_row = self.lines.len().saturating_sub(1);
        self.cursor_col = self
            .cursor_col
            .min(line_len_chars(&self.lines[self.cursor_row]));
    }

    pub fn page_up(&mut self, rows: usize) {
        let amount = rows.max(1);
        self.cursor_row = self.cursor_row.saturating_sub(amount);
        self.cursor_col = self
            .cursor_col
            .min(line_len_chars(&self.lines[self.cursor_row]));
    }

    pub fn page_down(&mut self, rows: usize) {
        let amount = rows.max(1);
        self.cursor_row = self
            .cursor_row
            .saturating_add(amount)
            .min(self.lines.len().saturating_sub(1));
        self.cursor_col = self
            .cursor_col
            .min(line_len_chars(&self.lines[self.cursor_row]));
    }

    pub fn find(&self, query: &str) -> Vec<MatchPos> {
        if query.is_empty() {
            return Vec::new();
        }
        let mut out = Vec::new();
        let query_chars = query.chars().count();
        for (row, line) in self.lines.iter().enumerate() {
            for (byte_start, _) in line.match_indices(query) {
                let start_col = line[..byte_start].chars().count();
                out.push(MatchPos {
                    row,
                    start_col,
                    end_col: start_col + query_chars,
                });
            }
        }
        out
    }

    pub fn replace_at(&mut self, at: &MatchPos, replacement: &str) {
        if at.row >= self.lines.len() {
            return;
        }
        let line = &mut self.lines[at.row];
        let start = char_to_byte_idx(line, at.start_col);
        let end = char_to_byte_idx(line, at.end_col);
        line.replace_range(start..end, replacement);
        self.cursor_row = at.row;
        self.cursor_col = at.start_col + replacement.chars().count();
    }

    pub fn replace_all(&mut self, find: &str, replacement: &str) -> usize {
        if find.is_empty() {
            return 0;
        }
        let mut total = 0usize;
        for row in 0..self.lines.len() {
            let mut row_count = 0usize;
            loop {
                let Some(byte_start) = self.lines[row].find(find) else {
                    break;
                };
                let byte_end = byte_start + find.len();
                self.lines[row].replace_range(byte_start..byte_end, replacement);
                row_count += 1;
            }
            total += row_count;
        }
        total
    }

    pub fn wipe(&mut self) {
        for line in &mut self.lines {
            line.zeroize();
        }
        self.lines.clear();
        self.lines.push(String::new());
        self.cursor_row = 0;
        self.cursor_col = 0;
    }
}

fn line_len_chars(input: &str) -> usize {
    input.chars().count()
}

fn char_to_byte_idx(input: &str, col: usize) -> usize {
    input
        .char_indices()
        .nth(col)
        .map(|(idx, _)| idx)
        .unwrap_or(input.len())
}

fn wrap_line_to_width(line: String, max_cols: usize) -> Vec<String> {
    if max_cols == 0 {
        return vec![line];
    }

    let mut wrapped = Vec::new();
    let mut current = line;
    while line_len_chars(&current) > max_cols {
        let split_col = wrap_split_col(&current, max_cols).max(1);
        let split_byte = char_to_byte_idx(&current, split_col);
        let right = current.split_off(split_byte);
        wrapped.push(current);
        current = right;
    }
    wrapped.push(current);
    wrapped
}

fn wrap_split_col(line: &str, max_cols: usize) -> usize {
    let limit = max_cols.min(line_len_chars(line));
    let mut last_whitespace = None;
    for (idx, ch) in line.chars().take(limit).enumerate() {
        if ch.is_whitespace() {
            last_whitespace = Some(idx);
        }
    }
    if let Some(idx) = last_whitespace {
        return idx + 1;
    }
    limit
}

#[cfg(test)]
mod tests {
    use super::{MatchPos, TextBuffer};

    #[test]
    fn insert_char_updates_text_and_cursor() {
        let mut buf = TextBuffer::new();
        buf.insert_char('A');
        assert_eq!(buf.to_text(), "A");
        assert_eq!(buf.cursor(), (0, 1));
    }

    #[test]
    fn newline_splits_line() {
        let mut buf = TextBuffer::from_text("Hello");
        buf.set_cursor(0, 2);
        buf.insert_newline();
        assert_eq!(buf.to_text(), "He\nllo");
        assert_eq!(buf.cursor(), (1, 0));
    }

    #[test]
    fn backspace_joins_lines_across_boundary() {
        let mut buf = TextBuffer::from_text("Hello\nWorld");
        buf.set_cursor(1, 0);
        buf.backspace();
        assert_eq!(buf.to_text(), "HelloWorld");
        assert_eq!(buf.cursor(), (0, 5));
    }

    #[test]
    fn cursor_left_right_crosses_lines() {
        let mut buf = TextBuffer::from_text("ab\ncd");
        buf.set_cursor(1, 0);
        buf.move_left();
        assert_eq!(buf.cursor(), (0, 2));
        buf.move_right();
        assert_eq!(buf.cursor(), (1, 0));
    }

    #[test]
    fn cursor_up_down_clamps_column() {
        let mut buf = TextBuffer::from_text("abcdef\nxy");
        buf.set_cursor(0, 5);
        buf.move_down();
        assert_eq!(buf.cursor(), (1, 2));
        buf.move_up();
        assert_eq!(buf.cursor(), (0, 2));
    }

    #[test]
    fn find_returns_all_matches() {
        let buf = TextBuffer::from_text("alpha beta\nbeta alpha");
        let matches = buf.find("beta");
        assert_eq!(matches.len(), 2);
        assert_eq!(
            matches[0],
            MatchPos {
                row: 0,
                start_col: 6,
                end_col: 10
            }
        );
    }

    #[test]
    fn replace_at_updates_target_span() {
        let mut buf = TextBuffer::from_text("I like cats");
        let at = MatchPos {
            row: 0,
            start_col: 7,
            end_col: 11,
        };
        buf.replace_at(&at, "dogs");
        assert_eq!(buf.to_text(), "I like dogs");
    }

    #[test]
    fn replace_all_replaces_every_occurrence() {
        let mut buf = TextBuffer::from_text("ha ha\nha");
        let count = buf.replace_all("ha", "ho");
        assert_eq!(count, 3);
        assert_eq!(buf.to_text(), "ho ho\nho");
    }

    #[test]
    fn delete_merges_with_next_line_at_eol() {
        let mut buf = TextBuffer::from_text("abc\ndef");
        buf.set_cursor(0, 3);
        buf.delete();
        assert_eq!(buf.to_text(), "abcdef");
        assert_eq!(buf.cursor(), (0, 3));
    }

    #[test]
    fn page_movement_stays_in_bounds() {
        let mut buf = TextBuffer::from_text("a\nb\nc\nd");
        buf.set_cursor(2, 0);
        buf.page_down(20);
        assert_eq!(buf.cursor(), (3, 0));
        buf.page_up(20);
        assert_eq!(buf.cursor(), (0, 0));
    }

    #[test]
    fn insert_text_handles_newlines() {
        let mut buf = TextBuffer::new();
        buf.insert_text("one\ntwo");
        assert_eq!(buf.to_text(), "one\ntwo");
        assert_eq!(buf.cursor(), (1, 3));
    }

    #[test]
    fn paragraph_up_jumps_to_previous_block() {
        let mut buf = TextBuffer::from_text("one\ntwo\n\nthree\nfour\n\nfive");
        buf.set_cursor(6, 2);
        buf.move_paragraph_up();
        assert_eq!(buf.cursor(), (3, 2));
        buf.move_paragraph_up();
        assert_eq!(buf.cursor(), (0, 2));
    }

    #[test]
    fn paragraph_down_jumps_to_next_block() {
        let mut buf = TextBuffer::from_text("one\ntwo\n\nthree\nfour\n\nfive");
        buf.set_cursor(0, 1);
        buf.move_paragraph_down();
        assert_eq!(buf.cursor(), (3, 1));
        buf.move_paragraph_down();
        assert_eq!(buf.cursor(), (6, 1));
    }

    #[test]
    fn duplicate_current_line_inserts_copy_below() {
        let mut buf = TextBuffer::from_text("one\ntwo");
        buf.set_cursor(0, 2);
        buf.duplicate_current_line();
        assert_eq!(buf.to_text(), "one\none\ntwo");
        assert_eq!(buf.cursor(), (1, 2));
    }

    #[test]
    fn delete_current_line_keeps_buffer_non_empty() {
        let mut buf = TextBuffer::from_text("one\ntwo\nthree");
        buf.set_cursor(1, 1);
        buf.delete_current_line();
        assert_eq!(buf.to_text(), "one\nthree");
        assert_eq!(buf.cursor(), (1, 1));
    }

    #[test]
    fn move_line_up_swaps_with_previous_line() {
        let mut buf = TextBuffer::from_text("one\ntwo\nthree");
        buf.set_cursor(1, 2);
        buf.move_line_up();
        assert_eq!(buf.to_text(), "two\none\nthree");
        assert_eq!(buf.cursor(), (0, 2));
    }

    #[test]
    fn move_line_down_swaps_with_next_line() {
        let mut buf = TextBuffer::from_text("one\ntwo\nthree");
        buf.set_cursor(1, 2);
        buf.move_line_down();
        assert_eq!(buf.to_text(), "one\nthree\ntwo");
        assert_eq!(buf.cursor(), (2, 2));
    }

    #[test]
    fn blank_line_insertions_place_cursor_on_new_line() {
        let mut buf = TextBuffer::from_text("one\ntwo");
        buf.set_cursor(1, 1);
        buf.insert_blank_line_above();
        assert_eq!(buf.to_text(), "one\n\ntwo");
        assert_eq!(buf.cursor(), (1, 0));

        buf.insert_blank_line_below();
        assert_eq!(buf.to_text(), "one\n\n\ntwo");
        assert_eq!(buf.cursor(), (2, 0));
    }

    #[test]
    fn move_to_top_and_bottom_jump_to_buffer_extremes() {
        let mut buf = TextBuffer::from_text("one\ntwo\nthree");
        buf.set_cursor(1, 2);
        buf.move_to_bottom();
        assert_eq!(buf.cursor(), (2, 2));
        buf.move_to_top();
        assert_eq!(buf.cursor(), (0, 2));
    }

    #[test]
    fn wrap_current_line_splits_long_word() {
        let mut buf = TextBuffer::from_text("abcdefghij");
        buf.set_cursor(0, 10);
        buf.wrap_current_line(6);
        assert_eq!(buf.to_text(), "abcdef\nghij");
        assert_eq!(buf.cursor(), (1, 4));
    }

    #[test]
    fn wrap_current_line_prefers_whitespace_boundary() {
        let mut buf = TextBuffer::from_text("hello world");
        buf.set_cursor(0, 11);
        buf.wrap_current_line(10);
        assert_eq!(buf.to_text(), "hello \nworld");
        assert_eq!(buf.cursor(), (1, 5));
    }

    #[test]
    fn wrap_current_line_keeps_cursor_within_wrapped_segment() {
        let mut buf = TextBuffer::from_text("alpha beta gamma");
        buf.set_cursor(0, 7);
        buf.wrap_current_line(8);
        assert_eq!(buf.to_text(), "alpha \nbeta \ngamma");
        assert_eq!(buf.cursor(), (1, 1));
    }
}
