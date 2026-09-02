use super::{Call, Future};

use iced::widget::{container, text};
use iced::{Element, Fill, Never};

use serde::Deserialize;

use std::path::Path;

#[derive(Deserialize)]
pub struct Bash {
    command: String,
}

impl Call for Bash {
    fn view(&self) -> Option<Element<'_, Never>> {
        Some(
            container(text(&self.command).size(14))
                .width(Fill)
                .padding(10)
                .style(container::dark)
                .into(),
        )
    }

    fn run(&self, project: &Path) -> Future {
        let command = self.command.clone();
        let project = project.to_path_buf();

        Box::pin(async move {
            let command = format!("exec 2>&1; {}", command);

            let output = tokio::process::Command::new("bash")
                .args(["-c", &command])
                .current_dir(project)
                .output()
                .await?;

            let stdout = String::from_utf8_lossy(&output.stdout).into_owned();

            if output.status.success() {
                Ok(stdout)
            } else {
                Err(std::io::Error::other(format!(
                    "{status}\n{stdout}",
                    status = output.status
                )))?
            }
        })
    }
}
