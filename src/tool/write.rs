use super::{Call, Future};
use crate::file;

use iced::widget::{column, container, text};
use iced::{Element, Fill, Never};

use serde::Deserialize;

use std::borrow::Cow;
use std::path::Path;

/// How many lines of a write the preview shows; bounds the height of
/// the view, not the content of the call.
const PREVIEW_LINES: usize = 10;
/// How many bytes a line of the preview may hold before it is cut;
/// bounds the layout work of pathological writes, like minified files.
const PREVIEW_LINE_WIDTH: usize = 200;

#[derive(Deserialize)]
#[serde(from = "Arguments")]
pub struct Write {
    path: String,
    content: String,
    preview: Preview,
}

#[derive(Deserialize)]
struct Arguments {
    path: String,
    content: String,
}

impl From<Arguments> for Write {
    fn from(Arguments { path, content }: Arguments) -> Self {
        let preview = Preview::of(&content);

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
            let lines = preview
                .lines
                .iter()
                .map(|line| {
                    Some(
                        text(line)
                            .size(14)
                            .width(Fill)
                            .wrapping(text::Wrapping::None)
                            .ellipsis(text::Ellipsis::End),
                    )
                })
                .chain([preview
                    .notice
                    .as_ref()
                    .map(|notice| text(notice).size(14).style(text::secondary))])
                .map(Element::from);

            container(column(lines).width(Fill))
                .width(Fill)
                .padding(10)
                .style(container::dark)
                .into()
        })
    }

    fn run(&self, project: &Path) -> Future {
        let path = self.path.clone();
        let content = self.content.clone();
        let project = project.to_path_buf();

        Box::pin(async move {
            let path = project.join(&path);
            let _lock = file::lock(&path).await;

            tokio::fs::write(&path, content).await?;

            Ok(format!("[Wrote to {}]", path.display()))
        })
    }
}

/// A bounded, line-based preview of the content of a write.
struct Preview {
    /// The lines of the preview, each cut to `PREVIEW_LINE_WIDTH`.
    lines: Vec<String>,
    /// A notice of the full size of the content, present only when
    /// the preview is truncated.
    notice: Option<String>,
}

impl Preview {
    fn of(content: &str) -> Self {
        let mut lines = Vec::new();
        let mut total = 0;

        for line in content.lines() {
            total += 1;

            if lines.len() == PREVIEW_LINES {
                continue;
            }

            let mut line = line;
            let mut cut = false;

            if line.len() > PREVIEW_LINE_WIDTH {
                line = &line[..line.floor_char_boundary(PREVIEW_LINE_WIDTH)];
                cut = true;
            }

            let mut preview_line = line.to_owned();

            if cut {
                preview_line.push('…');
            }

            lines.push(preview_line);
        }

        let truncated = lines.len() < total;

        Self {
            lines,
            notice: truncated.then(|| format!("… ({total} lines, {} bytes total)", content.len())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{PREVIEW_LINE_WIDTH, PREVIEW_LINES, Preview, Write};

    #[test]
    fn parses_arguments_and_caches_preview() {
        let write: Write = serde_json::from_str(r#"{"path":"a.txt","content":"a\nb\nc"}"#).unwrap();

        assert_eq!(write.path, "a.txt");
        assert_eq!(write.content, "a\nb\nc");
        assert_eq!(write.preview.lines, ["a", "b", "c"]);
        assert!(write.preview.notice.is_none());
    }

    #[test]
    fn short_content_is_previewed_in_full() {
        let preview = Preview::of("a\nb\nc");

        assert_eq!(preview.lines, ["a", "b", "c"]);
        assert!(preview.notice.is_none());
        assert!(!preview.lines.is_empty());
    }

    #[test]
    fn empty_content_has_no_preview() {
        let preview = Preview::of("");

        assert!(preview.lines.is_empty());
        assert!(preview.notice.is_none());
    }

    #[test]
    fn a_long_write_is_cut_to_the_line_budget() {
        let total_lines = PREVIEW_LINES + 20;
        let content: String = (1..=total_lines)
            .map(|line| format!("line {line}\n"))
            .collect();

        let preview = Preview::of(&content);

        assert_eq!(preview.lines.len(), PREVIEW_LINES);
        assert_eq!(preview.lines.first().unwrap(), "line 1");

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
            "x".repeat(PREVIEW_LINE_WIDTH + 10),
            "y".repeat(PREVIEW_LINE_WIDTH + 10)
        );

        let preview = Preview::of(&content);

        assert_eq!(preview.lines.len(), 2);

        for line in &preview.lines {
            assert_eq!(line.len(), PREVIEW_LINE_WIDTH + '…'.len_utf8());
            assert!(line.ends_with('…'));
        }

        // Every line was shown; the notice only reports more lines.
        assert!(preview.notice.is_none());
    }

    #[test]
    fn cuts_do_not_split_multibyte_characters() {
        // Each character is 3 bytes, so a cut at the byte budget
        // would land in the middle of one.
        let content = "中".repeat(PREVIEW_LINE_WIDTH / 3 + 10);
        let preview = Preview::of(&content);

        assert_eq!(preview.lines.len(), 1);

        let line = preview.lines.first().unwrap();
        let cut = &line[..line.len() - '…'.len_utf8()];

        assert!(line.ends_with('…'));
        assert!(cut.chars().all(|c| c == '中'));
    }
}
