use crate::core::Project;
use crate::font;
use crate::highlight;
use crate::tool::Output;
use crate::tool::call::{self, Call};

use iced::widget::{column, rich_text, span, text};
use iced::{Element, Never};

use serde::Deserialize;
use tokio::io::AsyncBufReadExt;

use std::borrow::Cow;
use std::path::Path;

/// How many bytes a line of the command preview may hold before it
/// is cut; long, single-line commands are everyday, and a command's
/// length carries meaning in a way a file's does not, so the budget
/// is roomier than the write preview's default.
const PREVIEW_LINE_WIDTH: usize = 500;

/// How many characters a command's title may hold before it is cut;
/// it sits in the header next to the tool's label, where an
/// overlong one would crowd the message.
const TITLE_WIDTH: usize = 60;

#[derive(Deserialize)]
#[serde(from = "Arguments")]
pub struct Bash {
    command: String,
    title: Option<String>,
    preview: highlight::Preview,
}

#[derive(Deserialize)]
struct Arguments {
    command: String,
    #[serde(default, rename = "description")]
    title: Option<String>,
}

impl From<Arguments> for Bash {
    fn from(arguments: Arguments) -> Self {
        /// A title, sanitized for the header: surrounding
        /// whitespace is trimmed, an empty one is dropped, and an
        /// overlong one is cut at the last word boundary within
        /// `TITLE_WIDTH`, with an ellipsis for what fell off — or
        /// at the character boundary when no word boundary is at
        /// hand.
        fn sanitize_title(title: &str) -> Option<String> {
            let title = title.trim();

            if title.is_empty() {
                return None;
            }

            let end = title.floor_char_boundary(TITLE_WIDTH);

            if end < title.len() {
                let cut = title[..end].rfind(char::is_whitespace).unwrap_or(end);

                return Some(format!("{}…", &title[..cut]));
            }

            Some(title.to_owned())
        }

        let mut preview = highlight::Preview::new("bash", &arguments.command, PREVIEW_LINE_WIDTH);

        if let Some(first) = preview.lines.first_mut() {
            first.insert(0, span("$ "));
        }

        Self {
            command: arguments.command,
            title: arguments.title.as_deref().and_then(sanitize_title),
            preview,
        }
    }
}

impl Call for Bash {
    fn title(&self, _project: &Project) -> Option<Cow<'_, str>> {
        self.title.as_deref().map(Cow::Borrowed)
    }

    fn view(&self) -> Option<Element<'_, Never>> {
        let notice = self
            .preview
            .notice
            .as_ref()
            .map(|notice| text(notice).size(font::SMALL).style(text::secondary));

        let lines = self
            .preview
            .lines
            .iter()
            .map(|line| rich_text(line).size(font::SMALL).into())
            .chain(notice.into_iter().map(Element::from));

        Some(column(lines).padding(10).into())
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
    use iced::Theme;
    use sipper::Sipper;

    /// The first line of the preview carries the plain "$ " prompt,
    /// and the command is highlighted by the bash grammar.
    #[test]
    fn the_preview_carries_a_prompt_and_uses_the_bash_grammar() {
        let bash = Bash::from(Arguments {
            command: "export FOO=bar".to_owned(),
            title: None,
        });

        let [line] = &bash.preview.lines[..] else {
            unreachable!()
        };

        let prompt = line.first().unwrap();
        assert_eq!(prompt.text.as_ref(), "$ ");
        assert!(prompt.color.is_none());

        // The keyword and the variable take the colors of the theme.
        assert!(line.iter().any(|span| {
            span.text.as_ref() == "export"
                && span.color == Some(Theme::CatppuccinMocha.palette().primary.base.color)
        }));

        assert!(line.iter().any(|span| {
            span.text.as_ref() == "FOO"
                && span.color == Some(Theme::CatppuccinMocha.palette().danger.base.color)
        }));
    }

    /// A command line longer than the write preview's default budget
    /// but shorter than the bash budget is not cut.
    #[test]
    fn a_long_command_line_is_not_cut_at_the_write_preview_budget() {
        let command = "x".repeat(highlight::Preview::MAX_LINE_WIDTH + 50);

        let bash = Bash::from(Arguments {
            command,
            title: None,
        });

        assert_eq!(bash.preview.lines.len(), 1);
        assert!(
            bash.preview.lines[0]
                .iter()
                .all(|span| span.text.as_ref() != "…")
        );
        assert!(bash.preview.notice.is_none());
    }

    /// A title is trimmed before it is shown, and dropped
    /// altogether when it is empty.
    #[test]
    fn a_title_is_trimmed_and_dropped_when_empty() {
        let project = Project::current_dir().expect("cwd");
        let bash = Bash::from(Arguments {
            command: "ls".to_owned(),
            title: Some("  List the files  ".to_owned()),
        });

        assert_eq!(bash.title(&project).as_deref(), Some("List the files"));

        for empty in ["", "   "] {
            let bash = Bash::from(Arguments {
                command: "ls".to_owned(),
                title: Some(empty.to_owned()),
            });

            assert_eq!(bash.title(&project), None);
        }
    }

    /// An overlong title is cut at the last word boundary within
    /// `TITLE_WIDTH`, with an ellipsis for what fell off; a title
    /// without a word boundary falls back to the character one.
    #[test]
    fn an_overlong_title_is_cut_at_a_word_boundary() {
        let project = Project::current_dir().expect("cwd");
        let bash = Bash::from(Arguments {
            command: "ls".to_owned(),
            title: Some(
                "install the dependencies, build the workspace, and verify that the tests pass"
                    .to_owned(),
            ),
        });

        assert_eq!(
            bash.title(&project).as_deref(),
            Some("install the dependencies, build the workspace, and verify…")
        );

        let bash = Bash::from(Arguments {
            command: "ls".to_owned(),
            title: Some("x".repeat(TITLE_WIDTH + 10)),
        });

        let expected = format!("{}…", "x".repeat(TITLE_WIDTH));
        assert_eq!(bash.title(&project).as_deref(), Some(expected.as_str()));
    }

    /// An absent title keeps the header to the tool's label.
    #[test]
    fn an_absent_title_yields_none() {
        let project = Project::current_dir().expect("cwd");
        let bash = Bash::from(Arguments {
            command: "ls".to_owned(),
            title: None,
        });

        assert_eq!(bash.title(&project), None);
    }

    #[tokio::test]
    async fn streams_lines_and_accumulates_output() {
        let bash = Bash::from(Arguments {
            command: "echo one; echo two; echo -n three".to_owned(),
            title: None,
        });

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
        let bash = Bash::from(Arguments {
            command: "for ((i=1; i<=1500; i++)); do echo $i; done".to_owned(),
            title: None,
        });

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
        use std::process;

        #[cfg(unix)]
        use std::os::unix::process::ExitStatusExt;

        #[cfg(windows)]
        use std::os::windows::process::ExitStatusExt;

        // The raw encoding of a plain "exit with code 1" differs
        // per platform: on Unix the code sits in the high byte of
        // the wait status, while on Windows the raw value is the
        // code itself.
        #[cfg(unix)]
        let exit_one = process::ExitStatus::from_raw(1 << 8);

        #[cfg(windows)]
        let exit_one = process::ExitStatus::from_raw(1);

        let bash = Bash::from(Arguments {
            command: "echo boom; exit 1".to_owned(),
            title: None,
        });

        let mut run = bash.run(Path::new("."));

        while run.sip().await.is_some() {}

        let output = run.await.expect("command ran");
        assert_eq!(
            output.to_string(),
            format!("boom\n[Command failed ({exit_one})]"),
        );
    }

    #[tokio::test]
    async fn a_command_without_output_yields_an_empty_output() {
        let bash = Bash::from(Arguments {
            command: "true".to_owned(),
            title: None,
        });

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
        let bash = Bash::from(Arguments {
            command: "kill -9 $$".to_owned(),
            title: None,
        });

        let mut run = bash.run(Path::new("."));

        while run.sip().await.is_some() {}

        let output = run.await.expect("command ran");
        assert!(output.to_string().contains("signal: 9"));
    }
}
