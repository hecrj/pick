use std::collections::VecDeque;

/// The default cap: the most lines kept verbatim at the head and
/// tail of the output; beyond this, the middle is dropped.
const DEFAULT_CAP: usize = 500;

/// A bounded, line-oriented capture of a tool's output.
///
/// The first `cap` lines are kept, as are the most recent;
/// everything in between is dropped, so an unbounded amount of
/// output cannot grow a session without bound.
#[derive(Debug, Clone)]
pub struct Output {
    cap: usize,
    head: Vec<String>,
    tail: VecDeque<String>,
    lines: usize,
    bytes: usize,
}

impl Output {
    pub fn new() -> Self {
        Self::with_cap(DEFAULT_CAP)
    }

    /// Creates an output that keeps at most `cap` lines verbatim at
    /// the head and tail; beyond this, the middle is dropped.
    pub fn with_cap(cap: usize) -> Self {
        Self {
            cap,
            head: Vec::new(),
            tail: VecDeque::new(),
            lines: 0,
            bytes: 0,
        }
    }

    pub fn push(&mut self, line: String) {
        self.lines += 1;
        self.bytes += line.len() + 1;

        if self.head.len() < self.cap {
            self.head.push(line);
        } else {
            self.tail.push_back(line);
        }

        if self.tail.len() > self.cap {
            self.tail.pop_front();
        }
    }

    /// Pushes a message the tool itself generates, bracketed: the
    /// bracket convention keeps such notices distinguishable from
    /// verbatim content in the model's tool response.
    pub fn push_notice(&mut self, notice: String) {
        self.push(format!("[{notice}]"))
    }

    /// The total number of lines pushed.
    pub fn lines(&self) -> usize {
        self.lines
    }

    /// The total bytes of every line pushed, newline included.
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    /// The number of lines dropped from the middle.
    pub fn lines_ellided(&self) -> usize {
        self.lines - self.head.len() - self.tail.len()
    }

    /// The first `n` lines, in reading order; `n` is capped at the
    /// output's cap.
    pub fn head(&self, n: usize) -> impl Iterator<Item = &str> {
        self.head.iter().take(n.min(self.cap)).map(String::as_str)
    }

    /// The most recent `n` lines, in reading order; `n` is capped at
    /// the output's cap.
    pub fn tail(&self, n: usize) -> impl Iterator<Item = &str> {
        let n = n.min(self.cap);

        // The most recent lines are the suffixes of the two buffers,
        // the tail's first and then the head's.
        let from_tail = self.tail.len().min(n);
        let from_head = (n - from_tail).min(self.head.len());

        self.tail
            .iter()
            .skip(self.tail.len() - from_tail)
            .chain(self.head[self.head.len() - from_head..].iter())
            .map(String::as_str)
    }

    /// All the retained lines, in reading order.
    pub fn all(&self) -> impl Iterator<Item = &str> {
        self.head.iter().chain(self.tail.iter()).map(String::as_str)
    }
}

impl std::fmt::Display for Output {
    // The kept lines, with a notice of the elided middle when the
    // output overflowed the head and tail.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let elided = self.lines_ellided();

        if elided > 0 {
            for (i, line) in self.head.iter().enumerate() {
                if i > 0 {
                    f.write_str("\n")?;
                }

                f.write_str(line)?;
            }

            f.write_str("\n")?;
            write!(f, "[... {elided} lines elided]")?;

            for line in &self.tail {
                f.write_str("\n")?;
                f.write_str(line)?;
            }
        } else {
            for (i, line) in self.head.iter().chain(self.tail.iter()).enumerate() {
                if i > 0 {
                    f.write_str("\n")?;
                }

                f.write_str(line)?;
            }
        }

        Ok(())
    }
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
}
