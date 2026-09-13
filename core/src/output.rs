use std::collections::VecDeque;

/// The default cap: the most lines kept verbatim at the head and
/// tail of the output; beyond this, the middle is dropped.
const DEFAULT_CAP: usize = 500;

/// The default byte cap: the most bytes kept across the head and
/// tail of the output; beyond this, the oldest kept lines are
/// dropped.
const DEFAULT_BYTE_CAP: usize = 64 * 1024;

/// The longest truncation notice: `[... 999999999 bytes elided]`.
const MAX_TRUNCATION_NOTICE: usize = 28;

/// A bounded, line-oriented capture of a tool's output.
///
/// The first `cap` lines are kept, as are the most recent;
/// everything in between is dropped. The kept content is
/// additionally bounded by a byte cap — notices excepted: a line
/// alone longer than it is truncated, and while the kept lines
/// exceed it the oldest are dropped first — so an unbounded amount
/// of output cannot grow a session without bound.
#[derive(Debug, Clone)]
pub struct Output {
    line_cap: usize,
    byte_cap: usize,
    head: VecDeque<String>,
    tail: VecDeque<String>,
    lines: usize,
    bytes: usize,
    retained: usize,
    front: usize,
}

impl Output {
    pub fn new() -> Self {
        Self::with_caps(DEFAULT_CAP, DEFAULT_BYTE_CAP)
    }

    /// Creates an output that keeps at most `cap` lines verbatim at
    /// the head and tail; beyond this, the middle is dropped.
    pub fn with_cap(cap: usize) -> Self {
        Self::with_caps(cap, DEFAULT_BYTE_CAP)
    }

    fn with_caps(line_cap: usize, byte_cap: usize) -> Self {
        Self {
            line_cap,
            byte_cap,
            head: VecDeque::new(),
            tail: VecDeque::new(),
            lines: 0,
            bytes: 0,
            retained: 0,
            front: 0,
        }
    }

    /// Appends a line to the output.
    ///
    /// A line alone longer than the byte cap is truncated, and
    /// while the kept lines exceed it the oldest kept lines are
    /// dropped.
    pub fn push(&mut self, line: String) {
        self.append(line, true);
    }

    /// Pushes a message the tool itself generates, bracketed: the
    /// bracket convention keeps such notices distinguishable from
    /// verbatim content in the model's tool response.
    ///
    /// Notices are exempt from the byte cap: they report on it, and
    /// must survive it.
    pub fn push_notice(&mut self, notice: String) {
        self.append(format!("[{notice}]"), false);
    }

    fn append(&mut self, line: String, bounded: bool) {
        self.lines += 1;
        self.bytes += line.len() + 1;

        // A line alone longer than the byte cap would defeat it, so
        // it is truncated to fit, newline included.
        let line = if bounded && line.len() + 1 > self.byte_cap {
            truncate(&line, self.byte_cap.saturating_sub(1))
        } else {
            line
        };

        self.retained += line.len() + 1;

        if self.head.len() < self.line_cap {
            self.head.push_back(line);
        } else {
            self.tail.push_back(line);

            if self.tail.len() > self.line_cap {
                let evicted = self
                    .tail
                    .pop_front()
                    .expect("the tail is bounded by the cap");
                self.retained -= evicted.len() + 1;
            }
        }

        // The byte cap bounds the kept lines as a whole, so under
        // pressure the oldest kept lines yield first: from the
        // head's front, and then the tail's.
        if bounded {
            while self.retained > self.byte_cap {
                let evicted = if self.head.is_empty() {
                    self.tail.pop_front()
                } else {
                    self.head.pop_front()
                }
                .expect("retained bytes imply a kept line");

                self.front += 1;
                self.retained -= evicted.len() + 1;
            }
        }
    }

    /// The total number of lines pushed.
    pub fn lines(&self) -> usize {
        self.lines
    }

    /// The total bytes of every line pushed, newline included.
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    /// The number of lines not kept: the elided middle, plus the
    /// oldest kept lines the byte cap dropped, if any.
    pub fn lines_ellided(&self) -> usize {
        self.lines - self.head.len() - self.tail.len()
    }

    /// The first `n` kept lines, in reading order; `n` is capped at
    /// the output's cap.
    pub fn head(&self, n: usize) -> impl Iterator<Item = &str> {
        self.head
            .iter()
            .take(n.min(self.line_cap))
            .map(String::as_str)
    }

    /// The most recent `n` lines, in reading order; `n` is capped at
    /// the output's cap.
    pub fn tail(&self, n: usize) -> impl Iterator<Item = &str> {
        let n = n.min(self.line_cap);

        // The most recent lines are the suffixes of the two buffers,
        // in reading order: the head's last, and then the tail's.
        let from_tail = self.tail.len().min(n);
        let from_head = (n - from_tail).min(self.head.len());

        self.head
            .iter()
            .skip(self.head.len() - from_head)
            .chain(self.tail.iter().skip(self.tail.len() - from_tail))
            .map(String::as_str)
    }

    /// All the retained lines, in reading order.
    pub fn all(&self) -> impl Iterator<Item = &str> {
        self.head.iter().chain(self.tail.iter()).map(String::as_str)
    }

    pub(crate) fn encode(&self) -> decoder::Value {
        use decoder::encode::{map, sequence, string, u64};

        map([
            ("line_cap", u64(self.line_cap as u64)),
            ("byte_cap", u64(self.byte_cap as u64)),
            ("head", sequence(string, &self.head)),
            ("tail", sequence(string, &self.tail)),
            ("lines", u64(self.lines as u64)),
            ("bytes", u64(self.bytes as u64)),
            ("retained", u64(self.retained as u64)),
            ("front", u64(self.front as u64)),
        ])
        .into_value()
    }

    pub(crate) fn decode(value: decoder::Value) -> decoder::Result<Self> {
        use decoder::decode::{map, sequence, string, u64};

        let mut fields = map(value)?;

        let head: VecDeque<String> = fields.required("head", sequence(string))?;
        let tail: VecDeque<String> = fields.required("tail", sequence(string))?;

        Ok(Self {
            line_cap: fields.required("line_cap", u64)? as usize,
            byte_cap: fields.required("byte_cap", u64)? as usize,
            head,
            tail,
            lines: fields.required("lines", u64)? as usize,
            bytes: fields.required("bytes", u64)? as usize,
            retained: fields.required("retained", u64)? as usize,
            front: fields.required("front", u64)? as usize,
        })
    }
}

impl Default for Output {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for Output {
    // The kept lines, with a notice of the elided middle and of the
    // oldest lines the byte cap dropped, when the output overflowed
    // the caps.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let front = self.front;
        let middle = self.lines_ellided() - front;

        if front > 0 {
            write!(f, "[... {front} lines elided]")?;
        }

        for (i, line) in self.head.iter().enumerate() {
            if i > 0 || front > 0 {
                f.write_str("\n")?;
            }

            f.write_str(line)?;
        }

        if middle > 0 {
            f.write_str("\n")?;
            write!(f, "[... {middle} lines elided]")?;
        }

        for line in &self.tail {
            f.write_str("\n")?;
            f.write_str(line)?;
        }

        Ok(())
    }
}

/// Truncates `line` to at most `budget` bytes, marking the dropped
/// bytes with a notice in the bracket convention.
fn truncate(line: &str, budget: usize) -> String {
    let keep = budget.saturating_sub(MAX_TRUNCATION_NOTICE);
    let at = line.floor_char_boundary(keep);
    let elided = line.len() - at;

    format!("{}[... {elided} bytes elided]", &line[..at])
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_CAP, Output};

    #[test]
    fn short_output_is_kept_in_full() {
        let mut output = Output::new();
        output.push("one".to_owned());
        output.push("two".to_owned());

        assert_eq!(output.lines(), 2);
        assert_eq!(output.bytes(), 8);
        assert_eq!(output.lines_ellided(), 0);
        assert_eq!(output.to_string(), "one\ntwo");
        assert_eq!(output.all().collect::<Vec<_>>(), ["one", "two"]);
        assert_eq!(Output::new().to_string(), "");
    }

    #[test]
    fn a_notice_is_pushed_bracketed() {
        let mut output = Output::new();
        output.push("hello".to_owned());
        output.push_notice("World is vast".to_owned());

        assert_eq!(output.to_string(), "hello\n[World is vast]");
        assert_eq!(output.lines(), 2);
    }

    #[test]
    fn a_long_output_keeps_only_its_head_and_tail() {
        let mut output = Output::new();
        for line in 0..(DEFAULT_CAP * 3) {
            output.push(format!("line {line}"));
        }

        assert_eq!(output.lines(), DEFAULT_CAP * 3);
        assert_eq!(output.lines_ellided(), DEFAULT_CAP);
        assert_eq!(output.head(1).collect::<Vec<_>>(), ["line 0"]);
        assert_eq!(
            output.head(DEFAULT_CAP).collect::<Vec<_>>().last().copied(),
            Some(format!("line {}", DEFAULT_CAP - 1).as_str())
        );
        assert_eq!(
            output
                .tail(DEFAULT_CAP)
                .collect::<Vec<_>>()
                .first()
                .copied(),
            Some(format!("line {}", DEFAULT_CAP * 2).as_str())
        );
        assert_eq!(
            output.tail(1).collect::<Vec<_>>(),
            [format!("line {}", DEFAULT_CAP * 3 - 1).as_str()]
        );
        assert_eq!(output.all().count(), DEFAULT_CAP * 2);
        // `n` is capped at `DEFAULT_CAP`.
        assert_eq!(
            output.head(DEFAULT_CAP * 3).collect::<Vec<_>>(),
            output.head(DEFAULT_CAP).collect::<Vec<_>>()
        );
        assert_eq!(
            output.tail(DEFAULT_CAP * 3).collect::<Vec<_>>(),
            output.tail(DEFAULT_CAP).collect::<Vec<_>>()
        );

        let rendered = output.to_string();
        assert!(rendered.starts_with("line 0\nline 1\n"));
        assert!(rendered.contains(&format!(
            "line 499\n[... {DEFAULT_CAP} lines elided]\nline {}",
            DEFAULT_CAP * 2
        )));
        assert!(rendered.ends_with(&format!("line {}", DEFAULT_CAP * 3 - 1)));
    }

    #[test]
    fn tail_lines_span_the_head_and_tail() {
        // The tail stays empty until the head is full, so the most
        // recent lines can live entirely in the head.
        let mut output = Output::new();
        for line in 0..10 {
            output.push(format!("line {line}"));
        }

        assert_eq!(output.head(2).collect::<Vec<_>>(), ["line 0", "line 1"]);
        assert_eq!(
            output.tail(3).collect::<Vec<_>>(),
            ["line 7", "line 8", "line 9"]
        );
        assert_eq!(output.tail(1).collect::<Vec<_>>(), ["line 9"]);

        // Overflow the head so the most recent lines sit in the tail.
        for line in 0..DEFAULT_CAP {
            output.push(format!("more {line}"));
        }

        assert_eq!(output.tail(2).collect::<Vec<_>>(), ["more 498", "more 499"]);
    }

    #[test]
    fn a_lowered_cap_bounds_the_head_and_tail() {
        let mut output = Output::with_cap(3);
        for line in 0..8 {
            output.push(format!("line {line}"));
        }

        assert_eq!(output.lines(), 8);
        assert_eq!(output.lines_ellided(), 2);
        // `n` is capped at the output's cap.
        assert_eq!(
            output.head(8).collect::<Vec<_>>(),
            ["line 0", "line 1", "line 2"]
        );
        assert_eq!(
            output.tail(8).collect::<Vec<_>>(),
            ["line 5", "line 6", "line 7"]
        );
        assert_eq!(
            output.to_string(),
            "line 0\nline 1\nline 2\n[... 2 lines elided]\nline 5\nline 6\nline 7"
        );
    }

    #[test]
    fn a_tail_spanning_both_buffers_is_in_reading_order() {
        // The head is full and the most recent lines span both
        // buffers: `tail` must keep reading order, the head's last
        // lines before the tail's.
        let mut output = Output::with_cap(5);
        for line in 0..8 {
            output.push(format!("line {line}"));
        }

        // head = [0..=4], tail = [5..=7]
        assert_eq!(
            output.tail(4).collect::<Vec<_>>(),
            ["line 4", "line 5", "line 6", "line 7"]
        );
        assert_eq!(
            output.tail(5).collect::<Vec<_>>(),
            ["line 3", "line 4", "line 5", "line 6", "line 7"]
        );
        // A request fully inside the tail is unaffected.
        assert_eq!(output.tail(2).collect::<Vec<_>>(), ["line 6", "line 7"]);
    }

    #[test]
    fn a_raised_cap_keeps_the_output_whole() {
        let mut output = Output::with_cap(8);
        for line in 0..8 {
            output.push(format!("line {line}"));
        }

        assert_eq!(output.lines_ellided(), 0);
        assert_eq!(
            output.to_string(),
            "line 0\nline 1\nline 2\nline 3\nline 4\nline 5\nline 6\nline 7"
        );
    }

    #[test]
    fn a_line_longer_than_the_byte_cap_is_truncated() {
        let mut output = Output::with_caps(DEFAULT_CAP, 40);
        output.push("x".repeat(100));

        assert_eq!(output.lines(), 1);
        // `bytes` still counts the line as pushed, in full.
        assert_eq!(output.bytes(), 101);
        // The kept line, newline included, stays within the cap.
        assert!(output.to_string().len() < 40);
        // 39 bytes fit 11 of the line, leaving room for the notice.
        assert!(output.to_string().starts_with(&"x".repeat(11)));
        assert!(output.to_string().ends_with("[... 89 bytes elided]"));
    }

    #[test]
    fn a_truncation_stays_on_a_char_boundary() {
        let mut output = Output::with_caps(DEFAULT_CAP, 40);
        // 200 bytes of two-byte characters.
        output.push("é".repeat(100));

        // 11 bytes fit 5 characters, so 190 bytes are elided.
        assert!(output.to_string().starts_with("ééééé"));
        assert!(output.to_string().ends_with("[... 190 bytes elided]"));
    }

    #[test]
    fn the_byte_cap_drops_the_oldest_kept_lines() {
        let mut output = Output::with_caps(DEFAULT_CAP, 25);
        for c in ['a', 'b', 'c'] {
            // 9 characters: 10 bytes, newline included.
            output.push(c.to_string().repeat(9));
        }

        // 30 kept bytes exceed the cap, so the oldest line yields,
        // and the notice of its elision leads the output.
        assert_eq!(output.lines(), 3);
        assert_eq!(output.lines_ellided(), 1);
        assert_eq!(
            output.to_string(),
            "[... 1 lines elided]\nbbbbbbbbb\nccccccccc"
        );
    }

    #[test]
    fn the_kept_lines_stay_within_the_byte_cap() {
        let mut output = Output::with_caps(DEFAULT_CAP, 30);
        // A line alone longer than the cap, then many small lines.
        output.push("x".repeat(100));
        for _ in 0..100 {
            output.push("d".to_owned());
        }

        assert_eq!(output.lines(), 101);
        // The truncated line and the oldest small lines yielded, so
        // the 15 newest small lines — 30 bytes — remain.
        assert_eq!(output.all().count(), 15);
        assert!(output.to_string().starts_with("[... 86 lines elided]"));
    }

    #[test]
    fn byte_pressure_keeps_the_tail() {
        let mut output = Output::with_caps(DEFAULT_CAP, 30);
        for _ in 0..100 {
            output.push("e".to_owned());
        }

        // 100 lines of 2 bytes: the 85 oldest drop, the 15 newest
        // remain.
        assert_eq!(output.lines_ellided(), 85);
        assert_eq!(output.head(2).collect::<Vec<_>>(), ["e", "e"]);
        assert_eq!(output.tail(2).collect::<Vec<_>>(), ["e", "e"]);
    }

    #[test]
    fn byte_pressure_elides_the_front_and_the_middle() {
        // 500 long lines fill the head exactly — 65,500 bytes, just
        // under the byte cap — and 700 more lines overflow the line
        // cap's middle: the elision lands in two regions, both
        // noticed.
        let mut output = Output::new();
        for _ in 0..DEFAULT_CAP {
            output.push("z".repeat(130));
        }

        for _ in 0..700 {
            output.push("a".to_owned());
        }

        assert!(output.front > 0);
        // The 200 unkept lines split between the front region the
        // byte cap dropped and the middle between the kept windows;
        // both elisions are noticed.
        assert_eq!(output.lines_ellided(), 200);
        let middle = output.lines_ellided() - output.front;
        assert!(middle > 0);

        let rendered = output.to_string();
        assert!(rendered.starts_with(&format!("[... {} lines elided]", output.front)));
        assert!(rendered.contains(&format!("[... {middle} lines elided]")));
        // The kept windows stay in reading order: the oldest kept
        // line is a head's, the newest a tail's.
        assert_eq!(output.head(1).next(), Some("z".repeat(130).as_str()));
        assert_eq!(output.tail(1).next(), Some("a"));
    }
}
