use super::{Call, Future};
use crate::file;
use crate::highlight;

use iced::border;
use iced::highlighter;
use iced::widget::text;
use iced::widget::{column, container, rich_text, scrollable, span};
use iced::{Color, Element, Fill, Fit, Highlighter, Never, Theme};

use serde::Deserialize;
use similar::{ChangeTag, InlineChangeOptions, TextDiff};

use std::borrow::Cow;
use std::ops::Range;
use std::path::Path;
use std::time::{Duration, Instant};

#[derive(Deserialize)]
#[serde(from = "Arguments")]
pub struct Edit {
    path: String,
    old_string: String,
    new_string: String,
    replace_all: bool,
    diff: Vec<Line>,
}

#[derive(Deserialize)]
struct Arguments {
    path: String,
    old_string: String,
    new_string: String,
    #[serde(default)]
    replace_all: bool,
}

impl From<Arguments> for Edit {
    fn from(
        Arguments {
            path,
            old_string,
            new_string,
            replace_all,
        }: Arguments,
    ) -> Self {
        let diff = Line::diff(&path, &old_string, &new_string);

        Self {
            path,
            old_string,
            new_string,
            replace_all,
            diff,
        }
    }
}

impl Call for Edit {
    fn title(&self) -> Option<Cow<'_, str>> {
        Some(Cow::Borrowed(&self.path))
    }

    fn view(&self) -> Option<Element<'_, Never>> {
        let lines = self.diff.iter().map(|line| {
            container(rich_text(line.spans.as_slice()).size(14))
                .width(Fill)
                .padding([2, 10])
                .style(|_theme| container::Style::default().background(line.background))
                .into()
        });

        Some(
            container(
                scrollable(column(lines).width(Fill))
                    .width(Fill)
                    .height(Fit.max(300))
                    .direction(scrollable::Direction::Vertical(
                        scrollable::Scrollbar::default().margin(10).spacing(0),
                    )),
            )
            .width(Fill)
            .padding([10, 0])
            .style(container::dark)
            .into(),
        )
    }

    fn run(&self, project: &Path) -> Future {
        let path = self.path.clone();
        let old_string = self.old_string.clone();
        let new_string = self.new_string.clone();
        let replace_all = self.replace_all;
        let project = project.to_path_buf();

        Box::pin(async move {
            let path = project.join(&path);
            let _lock = file::lock(&path).await;
            let contents = tokio::fs::read_to_string(&path).await?;

            if old_string.is_empty() {
                Err(std::io::Error::other("old_string must not be empty"))?
            }

            if old_string == new_string {
                Err(std::io::Error::other(
                    "old_string and new_string must be different",
                ))?
            }

            let occurrences = contents.matches(&old_string).count();

            if occurrences == 0 {
                Err(std::io::Error::other(format!(
                    "old_string not found in {}",
                    path.display()
                )))?
            }

            if occurrences > 1 && !replace_all {
                Err(std::io::Error::other(format!(
                    "old_string matches {occurrences} locations in {}; include more context to make it unique, or set replace_all to true",
                    path.display()
                )))?
            }

            let updated = if replace_all {
                contents.replace(&old_string, &new_string)
            } else {
                contents.replacen(&old_string, &new_string, 1)
            };

            tokio::fs::write(&path, updated).await?;

            if replace_all {
                Ok(format!(
                    "[Edited {} ({} replacements)]",
                    path.display(),
                    occurrences
                ))
            } else {
                Ok(format!("[Edited {} (1 replacement)]", path.display()))
            }
        })
    }
}

/// The total time budget for refining intraline changes.
const INLINE_DEADLINE: Duration = Duration::from_millis(10);
const BACKGROUND_ALPHA: f32 = 0.10;
const HIGHLIGHT_ALPHA: f32 = BACKGROUND_ALPHA * 2.0;

/// A line of the edit diff.
struct Line {
    /// The overall background of the line itself.
    background: Color,
    /// The spans of the line, used only for the inline highlights.
    spans: Vec<text::Span<'static>>,
}

impl Line {
    fn diff(path: &str, old: &str, new: &str) -> Vec<Self> {
        let palette = Self::palette(&Theme::CatppuccinMocha); // TODO: Pass `Theme` as argument
        let diff = TextDiff::from_lines(old, new);
        let options = InlineChangeOptions::default();
        let deadline = Some(Instant::now() + INLINE_DEADLINE);

        let settings = highlighter::Settings {
            token: highlight::token(path),
        };

        let mut old_highlighter = Highlighter::new(&settings);
        let mut new_highlighter = Highlighter::new(&settings);

        diff.iter_all_inline_changes_with_options_deadline(options, deadline)
            .map(|change| {
                let (prefix, style) = match change.tag() {
                    ChangeTag::Insert => ('+', Some(palette.added)),
                    ChangeTag::Delete => ('-', Some(palette.removed)),
                    ChangeTag::Equal => (' ', None),
                };

                let line: String = change.values().iter().map(|&(_, value)| value).collect();

                let scopes = match change.tag() {
                    ChangeTag::Insert => new_highlighter.highlight_line(&line),
                    ChangeTag::Delete => old_highlighter.highlight_line(&line),
                    ChangeTag::Equal => {
                        // The scopes are discarded, but the iterator must be
                        // consumed: the old highlighter's state only
                        // advances as the line is highlighted.
                        old_highlighter.highlight_line(&line).for_each(|_| {});
                        new_highlighter.highlight_line(&line)
                    }
                };

                Line::new(
                    prefix,
                    &line,
                    style,
                    change.values().iter().copied(),
                    scopes,
                )
            })
            .collect()
    }

    fn new<'a>(
        prefix: char,
        line: &str,
        style: Option<Color>,
        values: impl IntoIterator<Item = (bool, &'a str)>,
        scopes: impl IntoIterator<Item = (Range<usize>, highlighter::Scope)>,
    ) -> Self {
        let background = style
            .map(|color| color.scale_alpha(BACKGROUND_ALPHA))
            .unwrap_or(Color::TRANSPARENT);

        let mut spans = vec![span(format!("{prefix} "))];

        // Every line renders in its own `container`, so the terminator
        // is dropped from the end of the line, terminated or not.
        let mut end = line.len();

        if line.ends_with('\n') {
            end -= '\n'.len_utf8();

            if line[..end].ends_with('\r') {
                end -= '\r'.len_utf8();
            }
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
            .unwrap_or((line.len(), highlighter::Scope::Other));

        let highlight = style.map(|color| color.scale_alpha(HIGHLIGHT_ALPHA));

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
                            scope = highlighter::Scope::Other;
                        }
                    }
                }

                let stop = stop.min(range_end);
                let span = highlight::span(line, position..stop, scope);

                spans.push(span);

                position = stop;
            }
        }

        Self { background, spans }
    }

    fn palette(theme: &Theme) -> Palette {
        let palette = theme.seed();

        Palette {
            added: palette.success,
            removed: palette.danger,
        }
    }
}

struct Palette {
    added: Color,
    removed: Color,
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
    fn parses_arguments_and_caches_diff() {
        let edit: Edit =
            serde_json::from_str(r#"{"path":"README","old_string":"a\nb","new_string":"a\nc"}"#)
                .unwrap();

        assert_eq!(edit.path, "README");
        assert!(!edit.replace_all);

        let palette = Line::palette(&Theme::CatppuccinMocha);

        let [context_line, removed_line, added_line] = &edit.diff[..] else {
            unreachable!()
        };

        // The overall background of a line is a faded version of the
        // line's color; context lines are never emphasized, so they are
        // left transparent.
        assert_eq!(context_line.background, Color::TRANSPARENT);
        assert_eq!(
            removed_line.background,
            palette.removed.scale_alpha(BACKGROUND_ALPHA)
        );
        assert_eq!(
            added_line.background,
            palette.added.scale_alpha(BACKGROUND_ALPHA)
        );

        // Without a grammar for the path and without intraline
        // changes, the lines render with plain spans.
        for line in [context_line, removed_line, added_line] {
            for span in &line.spans {
                assert!(span.color.is_none());
                assert!(span.highlight.is_none());
            }
        }

        assert_eq!(text_of(context_line), "  a");
        assert_eq!(text_of(removed_line), "- b");
        assert_eq!(text_of(added_line), "+ c");
    }

    #[test]
    fn highlights_intraline_changes() {
        let palette = Line::palette(&Theme::CatppuccinMocha);

        let edit: Edit = serde_json::from_str(
            r#"{"path":"src/main.rs","old_string":"let x = 1\n","new_string":"let x = 2\n"}"#,
        )
        .unwrap();

        let [removed_line, added_line] = &edit.diff[..] else {
            unreachable!()
        };

        assert_eq!(
            removed_line.background,
            palette.removed.scale_alpha(BACKGROUND_ALPHA)
        );
        assert_eq!(
            added_line.background,
            palette.added.scale_alpha(BACKGROUND_ALPHA)
        );

        // The `+`/`-` markers stay plain; the color of the line is
        // carried by its background instead.
        let prefix = &removed_line.spans[0];
        assert_eq!(prefix.text.as_ref(), "- ");
        assert!(prefix.color.is_none());
        assert!(prefix.highlight.is_none());

        // The unchanged parts of the line are syntax-highlighted, but
        // never emphasized.
        assert!(removed_line.spans.iter().any(|span| {
            span.text.as_ref() == "let"
                && span.color == Some(Theme::CatppuccinMocha.palette().primary.strong.color)
                && span.highlight.is_none()
        }));

        // The changed character groups are highlighted on top of the
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
        assert_eq!(change.color, None);
        assert_eq!(
            change.highlight.map(|highlight| highlight.background),
            Some(palette.removed.scale_alpha(HIGHLIGHT_ALPHA).into())
        );

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
        assert_eq!(
            change.highlight.map(|highlight| highlight.background),
            Some(palette.added.scale_alpha(HIGHLIGHT_ALPHA).into())
        );
    }

    #[test]
    fn a_multi_line_diff_renders_a_line_per_file_line() {
        let edit: Edit = serde_json::from_str(
            r#"{"path":"a.txt","old_string":"a\nb\nc","new_string":"a\nx\ny\nz\nc"}"#,
        )
        .unwrap();

        let [context, removed, added_1, added_2, added_3, tail] = &edit.diff[..] else {
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
        let edit: Edit =
            serde_json::from_str(r#"{"path":"a.py","old_string":"x = \"","new_string":"x = 1"}"#)
                .unwrap();

        let [_, added] = &edit.diff[..] else {
            unreachable!()
        };

        let one = added
            .spans
            .iter()
            .find(|span| span.text.as_ref() == "1")
            .unwrap();

        assert_eq!(one.color, None);
    }

    #[test]
    fn a_line_is_highlighted_with_the_state_of_the_lines_before_it() {
        // The lines in the middle of a multi-line string are only colored
        // as string content if the parser state carries over from the
        // line that opened the string.
        let edit: Edit = serde_json::from_str(
            r#"{"path":"a.py","old_string":"","new_string":"x = \"\"\"\nhello\nworld\n\"\"\""}"#,
        )
        .unwrap();

        let [_, hello, world, _] = &edit.diff[..] else {
            unreachable!()
        };

        let string = Theme::CatppuccinMocha.palette().success.base.color;

        for line in [hello, world] {
            assert!(line.spans.iter().any(|span| span.color == Some(string)));
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
            let lines = Line::diff("README", old, new);

            for line in &lines {
                for span in &line.spans {
                    assert!(!span.text.contains('\n'));
                }
            }
        }
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
            let edit: Edit = serde_json::from_str(&format!(
                r#"{{"path":"src/main.rs","old_string":{},"new_string":{}}}"#,
                serde_json::to_string(old).unwrap(),
                serde_json::to_string(new).unwrap(),
            ))
            .unwrap();

            for line in &edit.diff {
                for span in &line.spans {
                    assert!(!span.text.is_empty(), "zero-width span");
                }
            }
        }
    }

    #[test]
    fn a_line_without_scopes_is_rendered_plain() {
        // The walk must not assume the highlighter scopes the whole
        // line: without any scopes, the line renders without syntax
        // highlighting instead of panicking.
        let line = Line::new(
            '-',
            "let x = 1\n",
            None,
            [(false, "let x = 1\n")],
            std::iter::empty(),
        );

        assert_eq!(text_of(&line), "- let x = 1");

        for span in &line.spans {
            assert!(span.color.is_none());
            assert!(span.highlight.is_none());
        }
    }

    #[test]
    fn scopes_that_stop_short_of_the_end_leave_the_remainder_plain() {
        let line = Line::new(
            '+',
            "let x = 1\n",
            None,
            [(false, "let x = 1\n")],
            [(0..3, highlighter::Scope::Keyword)],
        );

        assert_eq!(text_of(&line), "+ let x = 1");

        let keyword = Theme::CatppuccinMocha.palette().primary.strong.color;

        assert!(
            line.spans
                .iter()
                .any(|span| { span.text.as_ref() == "let" && span.color == Some(keyword) })
        );

        assert!(
            line.spans
                .iter()
                .any(|span| span.text.as_ref() == " x = 1" && span.color.is_none())
        );
    }

    #[test]
    fn an_empty_line_is_rendered_without_scopes() {
        let line = Line::new(' ', "", None, std::iter::empty(), std::iter::empty());

        assert_eq!(text_of(&line), "  ");
    }
}
