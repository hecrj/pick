use crate::core::file;
use crate::highlight;
use crate::tool::Output;
use crate::tool::call::{self, Call};

use iced::widget::{column, rich_text, text};
use iced::{Element, Fill, Never};

use serde::Deserialize;

use std::borrow::Cow;
use std::path::Path;

#[derive(Deserialize)]
#[serde(from = "Arguments")]
pub struct Write {
    path: String,
    content: String,
    preview: highlight::Preview,
}

#[derive(Deserialize)]
struct Arguments {
    path: String,
    content: String,
}

impl From<Arguments> for Write {
    fn from(Arguments { path, content }: Arguments) -> Self {
        let preview = highlight::Preview::file(&path, &content);

        Self {
            path,
            content,
            preview,
        }
    }
}

impl Call for Write {
    fn title(&self) -> Option<Cow<'_, str>> {
        Some(Cow::Borrowed(&self.path))
    }

    fn view(&self) -> Option<Element<'_, Never>> {
        let preview = &self.preview;

        (!preview.lines.is_empty()).then(|| {
            let notice = preview
                .notice
                .as_ref()
                .map(|notice| text(notice).size(14).style(text::secondary));

            let lines = preview
                .lines
                .iter()
                .map(|spans| {
                    rich_text(spans.as_slice())
                        .size(14)
                        .width(Fill)
                        .wrapping(text::Wrapping::None)
                        .ellipsis(text::Ellipsis::End)
                        .into()
                })
                .chain(notice.into_iter().map(Element::from));

            column(lines).width(Fill).padding(10).into()
        })
    }

    fn run(&self, project: &Path) -> call::Run {
        let path = self.path.clone();
        let content = self.content.clone();
        let project = project.to_path_buf();

        call::future(async move {
            let path = project.join(&path);
            let _lock = file::lock(&path).await;

            tokio::fs::write(&path, content).await?;

            let mut output = Output::new();
            output.push_notice(format!("Wrote to {}", path.display()));

            Ok(output)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::Theme;

    #[test]
    fn parses_arguments_and_caches_preview() {
        let write: Write = serde_json::from_str(r#"{"path":"a.txt","content":"a\nb\nc"}"#).unwrap();

        assert_eq!(write.path, "a.txt");
        assert_eq!(write.content, "a\nb\nc");
        assert_eq!(write.preview.lines.len(), 3);
        assert!(write.preview.notice.is_none());
    }

    #[test]
    fn highlights_the_preview_with_the_language_of_the_path() {
        let write: Write =
            serde_json::from_str(r#"{"path":"a.rs","content":"let s = \"hi\";"}"#).unwrap();

        let [line] = &write.preview.lines[..] else {
            unreachable!()
        };

        // The keyword and the string literal take the colors of the
        // theme, the rest of the line stays plain.
        assert!(line.iter().any(|span| {
            span.text.as_ref() == "let"
                && span.color == Some(Theme::CatppuccinMocha.palette().primary.base.color)
        }));

        assert!(line.iter().any(|span| {
            span.text.as_ref() == "hi"
                && span.color == Some(Theme::CatppuccinMocha.palette().success.base.color)
        }));

        let plain = line
            .iter()
            .find(|span| span.text.as_ref() == " s ")
            .unwrap();
        assert!(plain.color.is_none());
    }
}
