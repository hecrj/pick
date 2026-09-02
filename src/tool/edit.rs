use super::{Call, Future};

use iced::border;
use iced::widget::text;
use iced::widget::{column, container, rich_text, scrollable, span};
use iced::{Color, Element, Fill, Fit, Never, Theme};

use serde::Deserialize;
use similar::{ChangeTag, InlineChangeOptions, TextDiff};

use std::borrow::Cow;
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
        let diff = Line::diff(&old_string, &new_string);

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
                    "Edited {} ({} replacements)",
                    path.display(),
                    occurrences
                ))
            } else {
                Ok(format!("Edited {} (1 replacement)", path.display()))
            }
        })
    }
}

/// The total time budget for refining intraline changes.
const INLINE_DEADLINE: Duration = Duration::from_millis(10);
const BACKGROUND_ALPHA: f32 = 0.15;
const HIGHLIGHT_ALPHA: f32 = BACKGROUND_ALPHA * 2.0;

/// A line of the edit diff.
struct Line {
    /// The overall background of the line itself.
    background: Color,
    /// The spans of the line, used only for the inline highlights.
    spans: Vec<text::Span<'static>>,
}

impl Line {
    fn diff(old: &str, new: &str) -> Vec<Self> {
        let (added, removed) = Self::colors(&Theme::CatppuccinMocha); // TODO: Pass `Theme` as argument

        let diff = TextDiff::from_lines(old, new);
        let options = InlineChangeOptions::default();
        let deadline = Some(Instant::now() + INLINE_DEADLINE);

        diff.iter_all_inline_changes_with_options_deadline(options, deadline)
            .map(|change| {
                let (prefix, style) = match change.tag() {
                    ChangeTag::Insert => ('+', Some(added)),
                    ChangeTag::Delete => ('-', Some(removed)),
                    ChangeTag::Equal => (' ', None),
                };

                Line::new(prefix, style, change.values().iter().copied())
            })
            .collect()
    }

    fn new<'a>(
        prefix: char,
        style: Option<Color>,
        values: impl IntoIterator<Item = (bool, &'a str)>,
    ) -> Self {
        let background = style
            .map(|color| color.scale_alpha(BACKGROUND_ALPHA))
            .unwrap_or(Color::TRANSPARENT);

        let mut spans = vec![span(format!("{prefix} "))];

        // Merge the contiguous values of each emphasis into a single
        // segment as we go, looking ahead with `peek` to tell when the
        // current run ends.
        let mut values = values.into_iter().peekable();

        while let Some((emphasized, value)) = values.next() {
            let mut segment = String::new();
            segment.push_str(value);

            // Every line renders in its own `container`, so the
            // terminator is dropped from the content of the last
            // segment, terminated or not.
            if values.peek().is_none() && segment.ends_with('\n') {
                segment.pop();

                if segment.ends_with('\r') {
                    segment.pop();
                }
            }

            // The dropped terminator may leave the last segment empty,
            // and an empty span would render nothing anyway.
            if segment.is_empty() {
                continue;
            }

            spans.push(if emphasized && let Some(color) = style {
                span(segment)
                    .background(color.scale_alpha(HIGHLIGHT_ALPHA))
                    .border(border::rounded(2))
            } else {
                span(segment)
            });
        }

        Self { background, spans }
    }

    fn colors(theme: &Theme) -> (Color, Color) {
        let palette = theme.seed();

        (palette.success, palette.danger)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_arguments_and_caches_diff() {
        let edit: Edit = serde_json::from_str(
            r#"{"path":"src/main.rs","old_string":"a\nb","new_string":"a\nc"}"#,
        )
        .unwrap();

        assert_eq!(edit.path, "src/main.rs");
        assert!(!edit.replace_all);

        let (added, removed) = Line::colors(&Theme::CatppuccinMocha);

        let [context_line, removed_line, added_line] = &edit.diff[..] else {
            unreachable!()
        };

        // The overall background of a line is a faded version of the
        // line's color; context lines are never emphasized, so they are
        // left transparent.
        assert_eq!(context_line.background, Color::TRANSPARENT);
        assert_eq!(
            removed_line.background,
            removed.scale_alpha(BACKGROUND_ALPHA)
        );
        assert_eq!(added_line.background, added.scale_alpha(BACKGROUND_ALPHA));

        // The spans of a line are only used for the inline highlights,
        // so a line without intraline changes is rendered with plain
        // spans.
        for line in [context_line, removed_line, added_line] {
            for span in &line.spans {
                assert!(span.color.is_none());
                assert!(span.highlight.is_none());
            }
        }

        assert_eq!(context_line.spans.last().unwrap().text, "a");
        assert_eq!(removed_line.spans.last().unwrap().text, "b");
        assert_eq!(added_line.spans.last().unwrap().text, "c");
    }

    #[test]
    fn highlights_intraline_changes() {
        let (added, removed) = Line::colors(&Theme::CatppuccinMocha);

        let edit: Edit = serde_json::from_str(
            r#"{"path":"src/main.rs","old_string":"let x = 1\n","new_string":"let x = 2\n"}"#,
        )
        .unwrap();

        let [removed_line, added_line] = &edit.diff[..] else {
            unreachable!()
        };

        assert_eq!(
            removed_line.background,
            removed.scale_alpha(BACKGROUND_ALPHA)
        );
        assert_eq!(added_line.background, added.scale_alpha(BACKGROUND_ALPHA));

        let [removed_prefix, removed_context, removed_change] = &removed_line.spans[..] else {
            unreachable!()
        };

        // The `+`/`-` markers and the unchanged parts of the line stay
        // plain; the color of the line is carried by its background
        // instead.
        assert_eq!(removed_prefix.text, "- ");
        assert!(removed_prefix.color.is_none());
        assert!(removed_prefix.highlight.is_none());

        assert_eq!(removed_context.text, "let x = ");
        assert!(removed_context.color.is_none());
        assert!(removed_context.highlight.is_none());

        // The changed character groups are highlighted.
        assert_eq!(removed_change.text, "1");
        assert_eq!(removed_change.color, None);
        assert_eq!(
            removed_change
                .highlight
                .map(|highlight| highlight.background),
            Some(removed.scale_alpha(HIGHLIGHT_ALPHA).into())
        );

        let [added_prefix, added_context, added_change] = &added_line.spans[..] else {
            unreachable!()
        };

        assert_eq!(added_prefix.text, "+ ");
        assert!(added_prefix.color.is_none());
        assert!(added_prefix.highlight.is_none());

        assert_eq!(added_context.text, "let x = ");
        assert!(added_context.color.is_none());
        assert!(added_context.highlight.is_none());

        assert_eq!(added_change.text, "2");
        assert_eq!(added_change.color, None);
        assert_eq!(
            added_change.highlight.map(|highlight| highlight.background),
            Some(added.scale_alpha(HIGHLIGHT_ALPHA).into())
        );
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
            let lines = Line::diff(old, new);

            for line in &lines {
                for span in &line.spans {
                    assert!(!span.text.contains('\n'));
                }
            }
        }
    }
}
