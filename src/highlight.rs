use iced::highlighter;
use iced::widget::{self, text};
use iced::{Code, Font, Theme};

use std::ops::Range;
use std::path::Path;

/// A bounded, line-based preview of some content,
/// syntax-highlighted by language.
pub struct Preview {
    /// The lines of the preview, each cut to its line-width limit.
    pub lines: Vec<Vec<text::Span<'static>>>,
    /// A notice of the full size of the content, present only when
    /// the preview is truncated.
    pub notice: Option<String>,
}

impl Preview {
    /// How many lines a preview shows; bounds the height of the
    /// view, not the content of the call.
    pub const MAX_LINES: usize = 10;
    /// The default number of bytes a line of a preview may hold
    /// before it is cut; bounds the layout work of pathological
    /// content, like minified files.
    pub const MAX_LINE_WIDTH: usize = 200;

    /// A preview of the file at `path`, syntax-highlighted by the
    /// language of its extension.
    pub fn file(path: &str, content: &str) -> Self {
        Self::new(&token(path), content, Self::MAX_LINE_WIDTH)
    }

    /// A preview of `content`, syntax-highlighted as `language`,
    /// with each line cut to `line_width` bytes.
    pub fn new(language: &str, content: &str, line_width: usize) -> Self {
        let mut highlighter = highlighter::Parser::new(&highlighter::Settings {
            token: language.to_owned(),
        });

        let mut lines = Vec::new();
        let mut total = 0;

        for line in content.lines() {
            total += 1;

            if lines.len() == Self::MAX_LINES {
                continue;
            }

            let mut line = line;
            let mut cut = false;

            if line.len() > line_width {
                line = &line[..line.floor_char_boundary(line_width)];
                cut = true;
            }

            let mut spans: Vec<text::Span<'static>> = highlighter
                .parse_line(line)
                .map(|(range, code)| span(line, range, code))
                .collect();

            if cut {
                spans.push(widget::span("…"));
            }

            lines.push(spans);
        }

        let truncated = lines.len() < total;

        Self {
            lines,
            notice: truncated.then(|| format!("… ({total} lines, {} bytes total)", content.len())),
        }
    }
}

/// The highlight token of `path`: the extension of the file, which
/// picks the grammar the syntax highlighter uses.
pub(crate) fn token(path: impl AsRef<Path>) -> String {
    path.as_ref()
        .extension()
        .map_or(String::new(), |extension| {
            extension.to_string_lossy().into_owned()
        })
}

/// A span of a region of `line` styled by the theme for `scope`.
pub(crate) fn span(line: &str, range: Range<usize>, code: Code) -> text::Span<'static> {
    let style = code.highlight(&Theme::CatppuccinMocha);

    widget::span(line[range].to_owned())
        .color_maybe(style.color)
        .font_maybe(style.style.map(|style| Font {
            style,
            ..Font::MONOSPACE
        }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The joined text of the spans of a preview line.
    fn text_of(spans: &[text::Span<'_>]) -> String {
        spans.iter().map(|span| span.text.to_string()).collect()
    }

    #[test]
    fn short_content_is_previewed_in_full() {
        let preview = Preview::file("a.txt", "a\nb\nc");

        let lines: Vec<_> = preview.lines.iter().map(|line| text_of(line)).collect();
        assert_eq!(lines, ["a", "b", "c"]);
        assert!(preview.notice.is_none());
        assert!(!preview.lines.is_empty());
    }

    #[test]
    fn empty_content_has_no_preview() {
        let preview = Preview::file("a.txt", "");

        assert!(preview.lines.is_empty());
        assert!(preview.notice.is_none());
    }

    #[test]
    fn a_long_write_is_cut_to_the_line_budget() {
        let total_lines = Preview::MAX_LINES + 20;
        let content: String = (1..=total_lines)
            .map(|line| format!("line {line}\n"))
            .collect();

        let preview = Preview::file("a.txt", &content);

        assert_eq!(preview.lines.len(), Preview::MAX_LINES);
        assert_eq!(text_of(preview.lines.first().unwrap()), "line 1");

        // 7 bytes for `line 1` through `line 9`, 8 for the rest.
        let total_bytes = 9 * 7 + (total_lines - 9) * 8;

        assert_eq!(
            preview.notice.unwrap(),
            format!("… ({total_lines} lines, {total_bytes} bytes total)")
        );
    }

    #[test]
    fn long_lines_are_cut_to_the_width_budget() {
        let content = format!(
            "{}\n{}",
            "x".repeat(Preview::MAX_LINE_WIDTH + 10),
            "y".repeat(Preview::MAX_LINE_WIDTH + 10)
        );

        let preview = Preview::file("a.txt", &content);

        assert_eq!(preview.lines.len(), 2);

        for line in &preview.lines {
            let line = text_of(line);

            assert_eq!(line.len(), Preview::MAX_LINE_WIDTH + '…'.len_utf8());
            assert!(line.ends_with('…'));
        }

        // Every line was shown; the notice only reports more lines.
        assert!(preview.notice.is_none());
    }

    #[test]
    fn cuts_do_not_split_multibyte_characters() {
        // Each character is 3 bytes, so a cut at the byte budget
        // would land in the middle of one.
        let content = "中".repeat(Preview::MAX_LINE_WIDTH / 3 + 10);
        let preview = Preview::file("a.txt", &content);

        assert_eq!(preview.lines.len(), 1);

        let line = text_of(preview.lines.first().unwrap());
        let cut = &line[..line.len() - '…'.len_utf8()];

        assert!(line.ends_with('…'));
        assert!(cut.chars().all(|c| c == '中'));
    }
}
