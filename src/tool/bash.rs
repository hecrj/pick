use crate::tool::call::{self, Call};

use iced::widget::{container, text};
use iced::{Element, Fill, Never};

use serde::Deserialize;
use tokio::io::AsyncBufReadExt;

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

    fn run(&self, project: &Path) -> call::Run {
        let command = self.command.clone();
        let project = project.to_path_buf();

        call::straw(async move |mut sender| {
            let command = format!("exec 2>&1; {}", command);

            let mut child = tokio::process::Command::new("bash")
                .args(["-c", &command])
                .current_dir(project)
                .kill_on_drop(true)
                .stdout(std::process::Stdio::piped())
                .spawn()?;

            let stdout = child.stdout.take().expect("stdout is piped");
            let mut lines = tokio::io::BufReader::new(stdout).lines();

            let mut output = String::new();

            while let Some(line) = lines.next_line().await? {
                output.push_str(&line);
                output.push('\n');

                sender.send(line).await;
            }

            let status = child.wait().await?;

            if status.success() {
                Ok(output)
            } else {
                Err(std::io::Error::other(format!("{status}\n{output}",)))?
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sipper::Sipper;

    #[tokio::test]
    async fn streams_lines_and_accumulates_output() {
        let bash = Bash {
            command: "echo one; echo two; echo -n three".to_owned(),
        };

        let mut run = bash.run(Path::new("."));

        let mut lines = Vec::new();
        while let Some(line) = run.sip().await {
            lines.push(line);
        }

        assert_eq!(lines, ["one", "two", "three"]);

        let output = run.await.expect("command succeeded");
        assert_eq!(output, "one\ntwo\nthree\n");
    }

    #[tokio::test]
    async fn failure_includes_output() {
        let bash = Bash {
            command: "echo boom; exit 1".to_owned(),
        };

        let mut run = bash.run(Path::new("."));

        while run.sip().await.is_some() {}

        let error = run.await.expect_err("command failed");
        assert!(error.to_string().contains("boom"));
    }
}
