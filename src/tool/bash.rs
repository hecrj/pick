use crate::tool::Output;
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
            let shell = shell().await?;
            let command = format!("exec 2>&1; {}", command);

            let mut child = tokio::process::Command::new(shell)
                .args(["-c", &command])
                .current_dir(project)
                .kill_on_drop(true)
                .stdout(std::process::Stdio::piped())
                .spawn()?;

            let stdout = child.stdout.take().expect("stdout is piped");
            let mut lines = tokio::io::BufReader::new(stdout).lines();

            let mut output = Output::new();

            while let Some(line) = lines.next_line().await? {
                output.push(line.clone());

                sender.send(line).await;
            }

            let status = child.wait().await?;

            if !status.success() {
                output.push_notice(format!("Command failed ({status})"));
            }

            Ok(output)
        })
    }
}

/// Resolves the `bash` binary that runs the command.
///
/// On Windows the `bash` on the PATH may be the WSL stub, which
/// cannot run commands when no distribution is installed, so the
/// candidates are tried in turn and only a shell that can actually
/// run a command is trusted.
#[cfg(not(windows))]
async fn shell() -> Result<&'static str, std::io::Error> {
    Ok("bash")
}

#[cfg(windows)]
async fn shell() -> Result<&'static str, std::io::Error> {
    /// The `bash` binary, cached for the life of the process: a
    /// user's bash installation is not expected to change while
    /// the program runs. Failures are not cached, so a bash
    /// installed mid-session is picked up by the next call.
    static SHELL: tokio::sync::OnceCell<&'static str> = tokio::sync::OnceCell::const_new();

    if let Some(shell) = SHELL.get() {
        return Ok(*shell);
    }

    const CANDIDATES: &[&str] = &[
        "bash",
        r"C:\Program Files\Git\bin\bash.exe",
        r"C:\Program Files (x86)\Git\bin\bash.exe",
    ];

    for candidate in CANDIDATES {
        // A `bash` that cannot run a trivial command — like the WSL
        // stub without a distribution — is not one.
        let usable = tokio::process::Command::new(candidate)
            .args(["-c", "true"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .await
            .is_ok_and(|status| status.success());

        if usable {
            // A concurrent call may cache the same shell first.
            SHELL.set(*candidate).ok();
            return Ok(*candidate);
        }
    }

    Err(std::io::Error::other(
        "no usable bash found; install Git for Windows or a WSL distribution",
    ))
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
        assert_eq!(output.to_string(), "one\ntwo\nthree");
    }

    #[tokio::test]
    async fn long_output_is_capped_to_its_head_and_tail() {
        let bash = Bash {
            command: "for ((i=1; i<=1500; i++)); do echo $i; done".to_owned(),
        };

        let mut run = bash.run(Path::new("."));

        let mut streamed = 0;
        while run.sip().await.is_some() {
            streamed += 1;
        }

        assert_eq!(streamed, 1_500);

        let output = run.await.expect("command succeeded");

        assert_eq!(output.lines(), 1_500);
        assert_eq!(output.lines_ellided(), 500);

        let rendered = output.to_string();
        assert!(rendered.starts_with("1\n2\n"));
        assert!(rendered.contains("500\n[... 500 lines elided]\n1001"));
        assert!(rendered.ends_with("\n1499\n1500"));
    }

    #[tokio::test]
    async fn a_non_zero_exit_is_a_notice_not_an_error() {
        let bash = Bash {
            command: "echo boom; exit 1".to_owned(),
        };

        let mut run = bash.run(Path::new("."));

        while run.sip().await.is_some() {}

        let output = run.await.expect("command ran");
        assert_eq!(
            output.to_string(),
            "boom\n[Command failed (exit status: 1)]"
        );
    }

    #[tokio::test]
    async fn a_command_without_output_yields_an_empty_output() {
        let bash = Bash {
            command: "true".to_owned(),
        };

        let mut run = bash.run(Path::new("."));

        while run.sip().await.is_some() {}

        let output = run.await.expect("command ran");
        assert!(output.to_string().is_empty());
    }

    /// Windows has no signals, so a killed process cannot be reported
    /// as one; even under WSL the spawned child is the Windows proxy,
    /// whose exit status carries no signal.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_signaled_command_is_reported_as_a_notice() {
        let bash = Bash {
            command: "kill -9 $$".to_owned(),
        };

        let mut run = bash.run(Path::new("."));

        while run.sip().await.is_some() {}

        let output = run.await.expect("command ran");
        assert!(output.to_string().contains("signal: 9"));
    }
}
