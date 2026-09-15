use crate::Project;

use tokio::process;
use tokio::task;

use std::error;
use std::fmt;
use std::io;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct Status {
    pub branch: Branch,
    pub additions: u64,
    pub deletions: u64,
    pub untracked: Arc<[String]>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Branch {
    Unborn(String),
    Named(String),
    Detached(Sha),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Sha(String);

#[derive(Debug, Clone)]
pub struct File {
    pub path: String,
    pub content: String,
    pub lines: usize,
}

impl Status {
    pub fn current(project: &Project) -> impl Future<Output = Result<Self>> + 'static {
        let project = project.clone();

        async move {
            let output = git(&project, "status", ["--porcelain=v2", "-b", "-uall"]).await?;

            let mut oid = None;
            let mut branch = None;
            let mut untracked = Vec::new();

            for line in output.lines() {
                oid = oid.or_else(|| line.strip_prefix("# branch.oid "));
                branch = branch.or_else(|| line.strip_prefix("# branch.head "));

                if let Some(file) = line.strip_prefix("? ") {
                    untracked.push(file.to_owned());
                }
            }

            let branch = match (oid, branch) {
                (Some("(initial)"), Some(name)) => Branch::Unborn(name.to_owned()),
                (Some(sha), Some("(detached)")) => Branch::Detached(Sha(sha.to_owned())),
                (Some(_sha), Some(name)) => Branch::Named(name.to_owned()),
                _ => return Err(io::Error::other("git status returned no branch oid/head").into()),
            };

            let (additions, deletions) = if let Branch::Unborn(_) = branch {
                (0, 0)
            } else {
                let numstat = git(&project, "diff", ["HEAD", "--numstat"]).await?;

                numstat
                    .lines()
                    .filter_map(|line| {
                        let mut parts = line.split_whitespace();

                        let additions: u64 = parts.next()?.parse().ok()?;
                        let deletions: u64 = parts.next()?.parse().ok()?;

                        Some((additions, deletions))
                    })
                    .fold((0, 0), |(additions, deletions), (a, d)| {
                        (additions + a, deletions + d)
                    })
            };

            dbg!(Ok(Self {
                branch,
                additions,
                deletions,
                untracked: Arc::from(untracked),
            }))
        }
    }
}

async fn git<'a>(
    project: &Project,
    subcommand: &'static str,
    args: impl IntoIterator<Item = &'a str>,
) -> Result<String> {
    let output = process::Command::new("git")
        .current_dir(project)
        .args(
            [
                "-c",
                "core.quotePath=false",
                "--no-optional-locks",
                subcommand,
            ]
            .into_iter()
            .chain(args),
        )
        .kill_on_drop(true)
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(io::Error::other(format!("git {subcommand} failed: {stderr}")).into());
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[derive(Debug, Clone)]
pub struct Error(Arc<io::Error>);

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Self(Arc::new(error))
    }
}

impl From<task::JoinError> for Error {
    fn from(error: task::JoinError) -> Self {
        Self(Arc::new(io::Error::other(error)))
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl error::Error for Error {}

pub type Result<T> = ::core::result::Result<T, Error>;
