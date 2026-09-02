use super::{Call, Future};
use crate::file;

use iced::widget::{container, text};
use iced::{Element, Fill, Never};

use serde::Deserialize;

use std::borrow::Cow;
use std::path::Path;

#[derive(Deserialize)]
pub struct Write {
    path: String,
    content: String,
}

impl Call for Write {
    fn title(&self) -> Option<Cow<'_, str>> {
        Some(Cow::Borrowed(&self.path))
    }

    fn view(&self) -> Option<Element<'_, Never>> {
        Some(
            container(text(&self.content[..self.content.floor_char_boundary(50)]).size(14))
                .width(Fill)
                .padding(10)
                .style(container::dark)
                .into(),
        )
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
