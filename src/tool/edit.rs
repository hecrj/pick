use super::{Call, Future};

use serde::Deserialize;

use std::borrow::Cow;
use std::path::Path;

#[derive(Deserialize)]
pub struct Edit {
    path: String,
    old_string: String,
    new_string: String,
    #[serde(default)]
    replace_all: bool,
}

impl Call for Edit {
    fn title(&self) -> Option<Cow<'_, str>> {
        Some(Cow::Borrowed(&self.path))
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
