use crate::core::git;
use crate::font;
use crate::highlight;

use iced::border;
use iced::highlighter;
use iced::padding;
use iced::widget::{center, container, rich_text, row, span, text};
use iced::{Center, Code, Color, Element, Fill, Never, Pixels, Theme};

use similar::{ChangeTag, InlineChangeOptions, TextDiff};

use std::ops::Range;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The total time budget for refining intraline changes.
const INLINE_DEADLINE: Duration = Duration::from_millis(10);

#[derive(Debug, Clone)]
pub struct Diff {
    pub lines: Arc<[Line]>,
}

impl Diff {
    pub fn new(path: &str, old: &str, new: &str, background: Color) -> Self {
        let palette = Palette::new(&Theme::CatppuccinMocha, background); // TODO: Pass theme as argument
        let diff = TextDiff::from_lines(old, new);
        let options = InlineChangeOptions::default();
        let deadline = Some(Instant::now() + INLINE_DEADLINE);

        let settings = highlighter::Settings {
            token: highlight::token(path),
        };

        let mut old_parser = highlighter::Parser::new(&settings);
        let mut new_parser = highlighter::Parser::new(&settings);

        let lines = diff
            .iter_all_inline_changes_with_options_deadline(options, deadline)
            .map(|change| {
                let tag = match change.tag() {
                    ChangeTag::Insert => Tag::Addition,
                    ChangeTag::Delete => Tag::Deletion,
                    ChangeTag::Equal => Tag::Context,
                };

                let line: String = change.values().iter().map(|&(_, value)| value).collect();

                let scopes = match change.tag() {
                    ChangeTag::Insert => new_parser.parse_line(&line),
                    ChangeTag::Delete => old_parser.parse_line(&line),
                    ChangeTag::Equal => {
                        // The scopes are discarded, but the iterator must be
                        // consumed: the old highlighter's state only
                        // advances as the line is highlighted.
                        old_parser.parse_line(&line).for_each(|_| {});
                        new_parser.parse_line(&line)
                    }
                };

                Line::new(
                    tag,
                    &line,
                    change.values().iter().copied(),
                    scopes,
                    &palette,
                )
            })
            .collect();

        Self { lines }
    }

    pub fn from_hunk(path: &str, hunk: &git::Hunk, background: Color) -> Self {
        let old: String = hunk
            .lines
            .iter()
            .filter_map(|line| {
                if let git::Line::Deleted { text, .. } | git::Line::Context { text, .. } = line {
                    Some(format!("{text}\n"))
                } else {
                    None
                }
            })
            .collect();

        let new: String = hunk
            .lines
            .iter()
            .filter_map(|line| {
                if let git::Line::Added { text, .. } | git::Line::Context { text, .. } = line {
                    Some(format!("{text}\n"))
                } else {
                    None
                }
            })
            .collect();

        Self::new(path, &old, &new, background)
    }

    pub fn view(
        &self,
        mut start: Option<(usize, usize)>,
    ) -> impl Iterator<Item = Element<'_, Never>> {
        self.lines.iter().map(move |line| {
            let gutter = if let Some((old, new)) = &mut start {
                let (left, right) = match line.tag {
                    Tag::Context => (Some(*old), Some(*new)),
                    Tag::Addition => (None, Some(*new)),
                    Tag::Deletion => (Some(*old), None),
                };

                match line.tag {
                    Tag::Context => {
                        *old += 1;
                        *new += 1;
                    }
                    Tag::Addition => {
                        *new += 1;
                    }
                    Tag::Deletion => {
                        *old += 1;
                    }
                }

                fn number(n: Option<usize>, line: &Line) -> Element<'_, Never> {
                    center(n.map(|n| {
                        text(n)
                            .size(font::TINY)
                            .style(move |theme: &Theme| text::Style {
                                color: if let Tag::Context = line.tag {
                                    Some(theme.palette().secondary.base.color)
                                } else {
                                    None
                                },
                            })
                    }))
                    .height(20)
                    .width(40)
                    .into()
                }

                Some(row![number(left, line), number(right, line)])
            } else {
                None
            };

            let spans = container(
                rich_text(line.spans.as_slice())
                    .wrapping(text::Wrapping::WordOrGlyph)
                    .size(font::SMALL)
                    .line_height(Pixels((font::SMALL * 1.75).round())),
            )
            .width(Fill)
            .padding(padding::horizontal(10))
            .style(|_theme| {
                container::Style::default().background(
                    line.style
                        .map(|style| style.background)
                        .unwrap_or(Color::TRANSPARENT),
                )
            });

            container(row![gutter, spans].align_y(Center))
                .style(|_theme| {
                    container::Style::default().background(
                        line.style
                            .map(|style| style.gutter)
                            .unwrap_or(Color::TRANSPARENT),
                    )
                })
                .into()
        })
    }
}

/// A line of the edit diff.
#[derive(Debug)]
pub struct Line {
    /// The tag of the line.
    tag: Tag,
    /// The style of the line.
    style: Option<Style>,
    /// The spans of the line, used only for the inline highlights.
    spans: Vec<text::Span<'static>>,
}

#[derive(Debug, PartialEq, Eq)]
enum Tag {
    Context,
    Addition,
    Deletion,
}

impl Line {
    fn new<'a>(
        tag: Tag,
        line: &str,
        values: impl IntoIterator<Item = (bool, &'a str)>,
        scopes: impl IntoIterator<Item = (Range<usize>, Code)>,
        palette: &Palette,
    ) -> Self {
        let (prefix, style) = match tag {
            Tag::Context => (' ', None),
            Tag::Addition => ('+', Some(palette.addition)),
            Tag::Deletion => ('-', Some(palette.deletion)),
        };

        let mut spans = vec![span(format!("{prefix} "))];

        // Every line renders in its own `container`, so the
        // terminator, whether it is `\n`, `\r\n`, or `\r`, is
        // dropped from the end of the line, terminated or not.
        let mut end = line.len();

        if line.ends_with('\n') {
            end -= '\n'.len_utf8();

            if line[..end].ends_with('\r') {
                end -= '\r'.len_utf8();
            }
        } else if line.ends_with('\r') {
            end -= '\r'.len_utf8();
        }

        // The scopes partition the line from zero to its end, so a
        // single cursor walks the segments and the scopes, cutting
        // them at `end`. If the highlighter provides no scopes, or
        // stops short of the end of the line, the remainder is
        // rendered without syntax highlighting.
        let mut position = 0;
        let mut values = values.into_iter();
        let mut scopes = scopes.into_iter();
        let (mut range_end, mut scope) = scopes
            .next()
            .map(|(range, scope)| (range.end, scope))
            .unwrap_or((line.len(), Code::Other));

        let highlight = style.map(|style| style.highlight);

        while let Some((emphasized, mut value)) = values.next() {
            if let Some(highlight) = highlight
                && emphasized
            {
                let mut total = value.len();
                let mut is_over = true;

                // Unify all emphasized ranges
                for (emphasized, other) in values.by_ref() {
                    if !emphasized {
                        value = other;
                        is_over = false;
                        break;
                    }

                    total += other.len();
                }

                spans.push(
                    span(&line[position..position + total])
                        .background(highlight)
                        .border(border::rounded(2))
                        .padding(padding::horizontal(2))
                        .to_static(),
                );

                position += total;

                if is_over {
                    break;
                }
            }

            let stop = end.min(position + value.len());

            while position < stop {
                // Running out of scopes leaves the remainder of the
                // line without syntax highlighting.
                while range_end <= position {
                    match scopes.next() {
                        Some((range, new_scope)) => {
                            range_end = range.end;
                            scope = new_scope;
                        }
                        None => {
                            range_end = line.len();
                            scope = Code::Other;
                        }
                    }
                }

                let stop = stop.min(range_end);
                let span = highlight::span(line, position..stop, scope);

                spans.push(span);

                position = stop;
            }
        }

        // The walk stops at `end`, before the terminator, so the
        // iterator is left partially consumed. The highlighter
        // applies its scope operations lazily, as the iterator is
        // consumed, so the remainder must be drained: the trailing
        // operations close the scopes of the line (a `//` comment,
        // for instance), and, left behind, those scopes would carry
        // over to the next line highlighted by the same highlighter.
        scopes.for_each(|_| {});

        Self { tag, style, spans }
    }
}

struct Palette {
    addition: Style,
    deletion: Style,
}

impl Palette {
    fn new(theme: &Theme, background: Color) -> Self {
        let palette = theme.seed();

        Self {
            addition: Style::new(background, palette.success),
            deletion: Style::new(background, palette.danger),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Style {
    background: Color,
    gutter: Color,
    highlight: Color,
}

impl Style {
    pub fn new(background: Color, accent: Color) -> Self {
        use iced::theme::palette;

        let darkened = palette::darken(accent, 0.3);

        Self {
            background: darkened.mix(background, 0.97),
            gutter: darkened.mix(background, 0.85),
            highlight: palette::darken(accent.mix(background, 0.95), 0.02),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The joined text of the spans of a line.
    fn text_of(line: &Line) -> String {
        line.spans
            .iter()
            .map(|span| span.text.to_string())
            .collect()
    }

    #[test]
    fn a_diff_without_grammar_or_changes_renders_plain_lines() {
        let diff = Diff::new("README", "a\nb", "a\nc", Color::TRANSPARENT);

        let [context_line, removed_line, added_line] = &diff.lines[..] else {
            unreachable!()
        };

        // Each line carries the tag of its role in the diff.
        assert_eq!(context_line.tag, Tag::Context);
        assert_eq!(removed_line.tag, Tag::Deletion);
        assert_eq!(added_line.tag, Tag::Addition);

        assert_eq!(text_of(context_line), "  a");
        assert_eq!(text_of(removed_line), "- b");
        assert_eq!(text_of(added_line), "+ c");
    }

    #[test]
    fn highlights_intraline_changes() {
        let diff = Diff::new(
            "src/main.rs",
            "let x = 1\n",
            "let x = 2\n",
            Color::TRANSPARENT,
        );

        let [removed_line, added_line] = &diff.lines[..] else {
            unreachable!()
        };

        assert_eq!(removed_line.tag, Tag::Deletion);
        assert_eq!(added_line.tag, Tag::Addition);

        // The `+`/`-` markers are the first span of each line.
        assert_eq!(removed_line.spans[0].text.as_ref(), "- ");
        assert_eq!(added_line.spans[0].text.as_ref(), "+ ");

        // The changed character group is emphasized on top of the
        // syntax highlighting.
        let changes: Vec<_> = removed_line
            .spans
            .iter()
            .filter(|span| span.highlight.is_some())
            .collect();

        let [change] = &changes[..] else {
            unreachable!()
        };

        assert_eq!(change.text.as_ref(), "1");

        assert_eq!(text_of(removed_line), "- let x = 1");
        assert_eq!(text_of(added_line), "+ let x = 2");

        let changes: Vec<_> = added_line
            .spans
            .iter()
            .filter(|span| span.highlight.is_some())
            .collect();

        let [change] = &changes[..] else {
            unreachable!()
        };

        assert_eq!(change.text.as_ref(), "2");
    }

    #[test]
    fn a_multi_line_diff_renders_a_line_per_file_line() {
        let diff = Diff::new("a.txt", "a\nb\nc", "a\nx\ny\nz\nc", Color::TRANSPARENT);

        let [context, removed, added_1, added_2, added_3, tail] = &diff.lines[..] else {
            unreachable!()
        };

        assert_eq!(text_of(context), "  a");
        assert_eq!(text_of(removed), "- b");
        assert_eq!(text_of(added_1), "+ x");
        assert_eq!(text_of(added_2), "+ y");
        assert_eq!(text_of(added_3), "+ z");
        assert_eq!(text_of(tail), "  c");

        // The terminators are dropped from the spans entirely.
        for line in [context, removed, added_1, added_2, added_3, tail] {
            for span in &line.spans {
                assert!(!span.text.contains('\n'));
            }
        }
    }

    #[test]
    fn an_inserted_line_is_highlighted_against_the_new_file() {
        // The old line is an unterminated string, which would leave the
        // parser inside a string if the diff were highlighted in a single
        // pass. The inserted line must instead take the state of the new
        // file, where `1` is a number, not a string.
        let diff = Diff::new("a.py", "x = \"", "x = 1", Color::TRANSPARENT);

        let [removed, added] = &diff.lines[..] else {
            unreachable!()
        };

        // The string content of the old file is colored...
        assert!(removed.spans.iter().any(|span| span.color.is_some()));

        // ...but the `1` of the new file is not.
        let one = added
            .spans
            .iter()
            .find(|span| span.text.as_ref() == "1")
            .unwrap();

        assert!(one.color.is_none());
    }

    #[test]
    fn a_line_is_highlighted_with_the_state_of_the_lines_before_it() {
        // The lines in the middle of a multi-line string are only colored
        // as string content if the parser state carries over from the
        // line that opened the string.
        let diff = Diff::new(
            "a.py",
            "",
            "x = \"\"\"\nhello\nworld\n\"\"\"",
            Color::TRANSPARENT,
        );

        let [_, hello, world, _] = &diff.lines[..] else {
            unreachable!()
        };

        for line in [hello, world] {
            assert!(line.spans.iter().any(|span| span.color.is_some()));
        }
    }

    #[test]
    fn strips_line_terminators() {
        // Every line renders in its own `container`, so the terminators
        // are dropped from the spans entirely, terminated or not.
        for (old, new) in [
            ("foo 1", "foo 2"),
            ("foo 1\n", "foo 2\n"),
            ("foo 1\nbar\nbaz", "foo 2\nbar\nqux\n"),
        ] {
            let diff = Diff::new("README", old, new, Color::TRANSPARENT);

            for line in diff.lines.iter() {
                for span in &line.spans {
                    assert!(!span.text.contains('\n'));
                }
            }
        }
    }

    #[test]
    fn strips_lone_cr_terminators() {
        // A lone `\r` terminates a line just like `\n` and `\r\n`,
        // so it is dropped from the spans along with them.
        let diff = Diff::new(
            "a.txt",
            "foo 1\rbar\rbaz\r",
            "foo 2\rbar\rqux\r",
            Color::TRANSPARENT,
        );

        let [removed_1, added_1, context, removed_2, added_2] = &diff.lines[..] else {
            unreachable!()
        };

        assert_eq!(text_of(removed_1), "- foo 1");
        assert_eq!(text_of(added_1), "+ foo 2");
        assert_eq!(text_of(context), "  bar");
        assert_eq!(text_of(removed_2), "- baz");
        assert_eq!(text_of(added_2), "+ qux");
    }

    #[test]
    fn scope_boundaries_never_emit_empty_spans() {
        // The walk syncs the scope cursor with the position cursor
        // before cutting each span, so a cursor sitting on a scope
        // boundary must never yield a zero-width span.
        for (old, new) in [
            ("let x = 1\n", "let x = 2\n"),
            ("let x = 1; y\n", "let x = 2; y\n"),
            ("let x = 1\r\n", "let x = 2\r\n"),
            (
                "fn main() {\n    let x = 1;\n}\n",
                "fn main() {\n    let x = 2;\n}\n",
            ),
        ] {
            let diff = Diff::new("src/main.rs", old, new, Color::TRANSPARENT);

            for line in diff.lines.iter() {
                for span in &line.spans {
                    assert!(!span.text.is_empty(), "zero-width span");
                }
            }
        }
    }

    #[test]
    fn a_line_without_scopes_is_rendered_plain() {
        let palette = Palette::new(&Theme::CatppuccinMocha, Color::TRANSPARENT);

        // The walk must not assume the highlighter scopes the whole
        // line: without any scopes, the line still renders instead
        // of panicking.
        let line = Line::new(
            Tag::Deletion,
            "let x = 1\n",
            [(false, "let x = 1\n")],
            std::iter::empty(),
            &palette,
        );

        assert_eq!(text_of(&line), "- let x = 1");
    }

    #[test]
    fn scopes_that_stop_short_of_the_end_leave_the_remainder_plain() {
        let palette = Palette::new(&Theme::CatppuccinMocha, Color::TRANSPARENT);

        let line = Line::new(
            Tag::Addition,
            "let x = 1\n",
            [(false, "let x = 1\n")],
            [(0..3, Code::Keyword)],
            &palette,
        );

        assert_eq!(text_of(&line), "+ let x = 1");
    }

    #[test]
    fn an_empty_line_is_rendered_without_scopes() {
        let palette = Palette::new(&Theme::CatppuccinMocha, Color::TRANSPARENT);

        let line = Line::new(
            Tag::Context,
            "",
            std::iter::empty(),
            std::iter::empty(),
            &palette,
        );

        assert_eq!(text_of(&line), "  ");
    }

    #[test]
    fn a_scope_closed_at_line_end_does_not_carry_over() {
        let diff = Diff::new(
            "src/main.rs",
            "// a comment\nlet x = 1;\n",
            "// a comment\nlet x = 2;\n",
            Color::TRANSPARENT,
        );

        let [context, removed, added] = &diff.lines[..] else {
            unreachable!()
        };

        // The comment is highlighted, and its scope closes at the end
        // of the line.
        assert!(
            context
                .spans
                .iter()
                .any(|span| span.text.as_ref() == " a comment" && span.color.is_some())
        );

        // The scope iterator is consumed lazily by the highlighter, and
        // the span walk stops before the line terminator. It must be
        // drained, or the scope that closed at the end of the comment
        // line stays open, and the uncolored regions of the lines after
        // it (the whitespace around the identifier) inherit its color.
        for line in [removed, added] {
            assert!(
                line.spans
                    .iter()
                    .any(|span| span.text.as_ref() == " x " && span.color.is_none())
            );
        }
    }
}
