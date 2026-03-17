#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatchPos {
    pub row: usize,
    pub start_col: usize,
    pub end_col: usize,
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

    pub fn move_to_line_start(&mut self) {
        self.cursor_col = 0;
    }

    pub fn move_to_line_end(&mut self) {
        self.cursor_col = line_len_chars(&self.lines[self.cursor_row]);
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
}
