use super::{Call, Future};

use iced::widget::text::Span;
use iced::widget::{container, rich_text, scrollable, span};
use iced::{Element, Fill, Fit, Never, Theme};

use serde::Deserialize;
use similar::{ChangeTag, TextDiff};

use std::borrow::Cow;
use std::path::Path;

#[derive(Deserialize)]
#[serde(from = "Arguments")]
pub struct Edit {
    path: String,
    old_string: String,
    new_string: String,
    replace_all: bool,
    diff: Vec<Span<'static>>,
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
        let diff = unified_diff(&old_string, &new_string);

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
        Some(
            container(
                scrollable(rich_text(self.diff.as_slice()).size(14))
                    .spacing(10)
                    .height(Fit.max(300)),
            )
            .width(Fill)
            .padding(10)
            .style(container::rounded_box)
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

fn unified_diff(old: &str, new: &str) -> Vec<Span<'static>> {
    let palette = Theme::CatppuccinMocha.palette();
    let added = palette.success.strong.color;
    let removed = palette.danger.strong.color;
    let context = palette.secondary.strong.color;

    let diff = TextDiff::from_lines(old, new);
    let mut changes = diff.iter_all_changes().peekable();
    let mut spans = Vec::new();

    while let Some(change) = changes.next() {
        let (prefix, color) = match change.tag() {
            ChangeTag::Insert => ('+', added),
            ChangeTag::Delete => ('-', removed),
            ChangeTag::Equal => (' ', context),
        };

        let mut line = format!("{prefix} {}", change.value());

        // Terminate every line but the last one so each change
        // renders on its own line.
        if changes.peek().is_some() && change.missing_newline() {
            line.push('\n');
        }

        spans.push(span(line).color(color).to_static());
    }

    spans
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

        let palette = Theme::CatppuccinMocha.palette();

        let [context, removed, added] = &edit.diff[..] else {
            unreachable!()
        };

        assert_eq!(context.text, "  a\n");
        assert_eq!(context.color, Some(palette.secondary.strong.color));

        assert_eq!(removed.text, "- b\n");
        assert_eq!(removed.color, Some(palette.danger.strong.color));

        assert_eq!(added.text, "+ c");
        assert_eq!(added.color, Some(palette.success.strong.color));
    }
}
