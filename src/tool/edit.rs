use crate::Diff;
use crate::core::Project;
use crate::core::file;
use crate::tool::call::{self, Call};
use crate::tool::{BACKGROUND, Output};

use iced::widget::{column, container, scrollable};
use iced::{Element, Fill, Fit, Never};

use serde::Deserialize;

use std::borrow::Cow;
use std::path::Path;

#[derive(Deserialize)]
#[serde(from = "Arguments")]
pub struct Edit {
    path: String,
    old_string: String,
    new_string: String,
    replace_all: bool,
    diff: Diff,
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
        let diff = Diff::new(&path, &old_string, &new_string, BACKGROUND);

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
    fn title(&self, project: &Project) -> Option<Cow<'_, str>> {
        Some(Cow::Owned(
            project.relative(&self.path).display().to_string(),
        ))
    }

    fn view(&self) -> Option<Element<'_, Never>> {
        Some(
            container(
                scrollable(column(self.diff.view()).width(Fill))
                    .width(Fill)
                    .height(Fit.max(300))
                    .direction(scrollable::Direction::Vertical(
                        scrollable::Scrollbar::default().margin(10).spacing(0),
                    )),
            )
            .padding([10, 0])
            .into(),
        )
    }

    fn run(&self, project: &Path) -> call::Run {
        let path = self.path.clone();
        let old_string = self.old_string.clone();
        let new_string = self.new_string.clone();
        let replace_all = self.replace_all;
        let project = project.to_path_buf();

        call::future(async move {
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
                    "old_string matches {occurrences} locations in {}; \
                     include more context to make it unique, or set replace_all to true",
                    path.display()
                )))?
            }

            let updated = if replace_all {
                contents.replace(&old_string, &new_string)
            } else {
                contents.replacen(&old_string, &new_string, 1)
            };

            tokio::fs::write(&path, updated).await?;

            let mut output = Output::new();
            output.push_notice(if replace_all {
                format!("Edited {} ({} replacements)", path.display(), occurrences)
            } else {
                format!("Edited {} (1 replacement)", path.display())
            });

            Ok(output)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use pick_test::Directory;

    /// Creates the project directory of a test, seeding it with files.
    fn project(test: &str, files: &[(&str, &str)]) -> Directory {
        let root = Directory::create(
            std::env::temp_dir()
                .join(format!("pick-edit-test-{}", std::process::id()))
                .join(test),
        )
        .unwrap();

        for (name, contents) in files {
            std::fs::write(root.join(name), contents).unwrap();
        }

        root
    }

    #[test]
    fn parses_arguments_and_caches_diff() {
        let edit: Edit =
            serde_json::from_str(r#"{"path":"README","old_string":"a\nb","new_string":"a\nc"}"#)
                .unwrap();

        assert_eq!(edit.path, "README");
        assert!(!edit.replace_all);

        // The diff of the strings is cached on the tool itself; the
        // rendering of its lines is covered in the `diff` module.
        assert_eq!(edit.diff.lines.len(), 3);
    }

    #[tokio::test]
    async fn a_unique_match_is_replaced_in_place() {
        let root = project("unique", &[("a.txt", "one\ntwo\n")]);

        let edit = Edit {
            path: "a.txt".to_owned(),
            old_string: "one".to_owned(),
            new_string: "uno".to_owned(),
            replace_all: false,
            diff: Diff::new("a.txt", "one", "uno", BACKGROUND),
        };

        let output = edit.run(&root).await.unwrap();

        assert_eq!(
            output.to_string(),
            format!("[Edited {} (1 replacement)]", root.join("a.txt").display())
        );
        assert_eq!(
            std::fs::read_to_string(root.join("a.txt")).unwrap(),
            "uno\ntwo\n"
        );
    }

    #[tokio::test]
    async fn an_ambiguous_match_fails_without_replace_all() {
        let root = project("ambiguous", &[("a.txt", "a\nb\na\nc\n")]);

        let edit = Edit {
            path: "a.txt".to_owned(),
            old_string: "a".to_owned(),
            new_string: "x".to_owned(),
            replace_all: false,
            diff: Diff::new("a.txt", "a", "x", BACKGROUND),
        };

        let error = edit.run(&root).await.unwrap_err();

        assert_eq!(
            error.to_string(),
            format!(
                "io operation failed: old_string matches 2 locations in {}; \
                 include more context to make it unique, or set replace_all to true",
                root.join("a.txt").display()
            )
        );

        assert_eq!(
            std::fs::read_to_string(root.join("a.txt")).unwrap(),
            "a\nb\na\nc\n"
        );
    }

    #[tokio::test]
    async fn replace_all_replaces_every_occurrence() {
        let root = project("replace-all", &[("a.txt", "a\nb\na\nc\na\n")]);

        let edit = Edit {
            path: "a.txt".to_owned(),
            old_string: "a".to_owned(),
            new_string: "x".to_owned(),
            replace_all: true,
            diff: Diff::new("a.txt", "a", "x", BACKGROUND),
        };

        let output = edit.run(&root).await.unwrap();

        assert_eq!(
            output.to_string(),
            format!("[Edited {} (3 replacements)]", root.join("a.txt").display())
        );
        assert_eq!(
            std::fs::read_to_string(root.join("a.txt")).unwrap(),
            "x\nb\nx\nc\nx\n"
        );
    }

    #[tokio::test]
    async fn a_missing_old_string_fails() {
        let root = project("missing", &[("a.txt", "one\ntwo\n")]);

        let edit = Edit {
            path: "a.txt".to_owned(),
            old_string: "three".to_owned(),
            new_string: "x".to_owned(),
            replace_all: false,
            diff: Diff::new("a.txt", "three", "x", BACKGROUND),
        };

        let error = edit.run(&root).await.unwrap_err();

        assert_eq!(
            error.to_string(),
            format!(
                "io operation failed: old_string not found in {}",
                root.join("a.txt").display()
            )
        );
        assert_eq!(
            std::fs::read_to_string(root.join("a.txt")).unwrap(),
            "one\ntwo\n"
        );
    }

    #[tokio::test]
    async fn an_empty_old_string_fails() {
        let root = project("empty-old", &[("a.txt", "one\n")]);

        let edit = Edit {
            path: "a.txt".to_owned(),
            old_string: String::new(),
            new_string: "two".to_owned(),
            replace_all: false,
            diff: Diff::new("a.txt", "", "two", BACKGROUND),
        };

        let error = edit.run(&root).await.unwrap_err();

        assert_eq!(
            error.to_string(),
            "io operation failed: old_string must not be empty"
        );
    }

    #[tokio::test]
    async fn an_identical_old_and_new_string_fails() {
        let root = project("identical", &[("a.txt", "one\n")]);

        let edit = Edit {
            path: "a.txt".to_owned(),
            old_string: "one".to_owned(),
            new_string: "one".to_owned(),
            replace_all: false,
            diff: Diff::new("a.txt", "one", "one", BACKGROUND),
        };

        let error = edit.run(&root).await.unwrap_err();

        assert_eq!(
            error.to_string(),
            "io operation failed: old_string and new_string must be different"
        );
    }
}
