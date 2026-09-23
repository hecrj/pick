use crate::Project;

use tokio::process;
use tokio::task;

use std::error;
use std::fmt;
use std::io;
use std::process::Stdio;
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

            Ok(Self {
                branch,
                additions,
                deletions,
                untracked: Arc::from(untracked),
            })
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Diff {
    pub files: Arc<[File]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct File {
    pub path: String,
    /// The path before a rename, if the file was renamed.
    pub original: Option<String>,
    pub state: State,
    /// Whether the file mode changed, e.g. the executable bit.
    pub mode_changed: bool,
    pub insertions: u64,
    pub deletions: u64,
    pub hunks: Arc<[Hunk]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Created,
    Modified,
    Deleted,
    ModeChanged,
    Binary,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    pub old: Range,
    pub new: Range,
    pub heading: Option<String>,
    pub lines: Arc<[Line]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Range {
    pub start: usize,
    pub count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line {
    Context {
        old: usize,
        new: usize,
        text: String,
    },
    Added {
        new: usize,
        text: String,
    },
    Deleted {
        old: usize,
        text: String,
    },
}

impl Diff {
    pub fn current(
        project: &Project,
        status: &Status,
    ) -> impl Future<Output = Result<Self>> + 'static {
        let project = project.clone();
        let status = status.clone();

        async move {
            // An unborn repo has no `HEAD`; the empty tree is the base
            // instead. `mktree` yields it for whatever object format the
            // repo uses, so nothing is hardcoded.
            let base = match &status.branch {
                Branch::Unborn(_) => git(&project, "mktree", []).await?.trim().to_owned(),
                _ => "HEAD".to_owned(),
            };

            let diff = git(
                &project,
                "diff",
                ["--no-color", "--no-ext-diff", "-U3", &base],
            )
            .await?;

            let mut files = Vec::new();

            // The file being built up.
            let mut path = None;
            let mut original = None;
            let mut state = State::Modified;
            let mut mode_changed = false;
            let mut insertions = 0;
            let mut deletions = 0;

            // The hunks being built up.
            let mut hunks = Vec::new();
            let mut hunk: Option<Hunk> = None;
            let mut lines = Vec::new();
            let mut old = 0;
            let mut new = 0;

            // Finalizes the pending hunk, if any. The line counters are
            // the only self-check the patch format offers; a drift means
            // a hunk body did not add up.
            macro_rules! close_hunk {
                () => {
                    if let Some(mut hunk) = hunk.take() {
                        debug_assert_eq!(
                            old,
                            hunk.old.start + hunk.old.count,
                            "the old side of a hunk in {path:?} does not add up"
                        );
                        debug_assert_eq!(
                            new,
                            hunk.new.start + hunk.new.count,
                            "the new side of a hunk in {path:?} does not add up"
                        );

                        hunk.lines = Arc::from(std::mem::take(&mut lines));
                        hunks.push(hunk);
                    }
                };
            }

            // Pushes the file being built up, if any. A mode change with
            // no hunks is *just* a mode change.
            macro_rules! push_file {
                () => {
                    if let Some(path) = path.take() {
                        files.push(File {
                            path,
                            original: original.take(),
                            state: if mode_changed && hunks.is_empty() {
                                State::ModeChanged
                            } else {
                                state
                            },
                            mode_changed,
                            insertions,
                            deletions,
                            hunks: Arc::from(std::mem::take(&mut hunks)),
                        });
                    }
                };
            }

            fn range(token: &str) -> Range {
                let token = token.strip_prefix(['-', '+']).unwrap_or(token);
                let mut parts = token.split(',');

                Range {
                    start: parts
                        .next()
                        .and_then(|part| part.parse().ok())
                        .unwrap_or_default(),
                    // The count is omitted when it is one.
                    count: parts.next().and_then(|part| part.parse().ok()).unwrap_or(1),
                }
            }

            fn text(line: &str) -> String {
                // CRLF source files carry a trailing `\r` in the patch.
                line.strip_suffix('\r').unwrap_or(line).to_owned()
            }

            for line in diff.lines() {
                if let Some(paths) = line.strip_prefix("diff --git ") {
                    close_hunk!();
                    push_file!();

                    // The operands are `a/<old> b/<new>`. Paths may contain
                    // spaces, so split on the last ` b/`; a new path that
                    // itself contains ` b/` would be misread, but that is
                    // a corner of a corner.
                    let (a, b) = paths.rsplit_once(" b/").unwrap_or(("", ""));
                    let a = a.strip_prefix("a/").unwrap_or(a);
                    let b = b.strip_prefix("b/").unwrap_or(b);

                    path = Some(b.to_owned());
                    original = (a != b).then(|| a.to_owned());
                    state = State::Modified;
                    mode_changed = false;
                    insertions = 0;
                    deletions = 0;
                }

                if line.starts_with("new file mode ") {
                    state = State::Created;
                } else if line.starts_with("deleted file mode ") {
                    state = State::Deleted;
                } else if line.starts_with("old mode ") || line.starts_with("new mode ") {
                    mode_changed = true;
                } else if line.starts_with("Binary files ") {
                    state = State::Binary;
                }

                if let Some(metadata) = line.strip_prefix("@@ ") {
                    close_hunk!();

                    let mut parts = metadata.split_whitespace();
                    let hunk_old = range(parts.next().unwrap_or_default());
                    let hunk_new = range(parts.next().unwrap_or_default());

                    // The last `@@` ends the header; anything after it is
                    // a best-effort section heading.
                    let heading = line
                        .rsplit_once("@@")
                        .map(|(_, heading)| heading.trim())
                        .filter(|heading| !heading.is_empty());

                    // Seed the line counters from the header.
                    old = hunk_old.start;
                    new = hunk_new.start;

                    hunk = Some(Hunk {
                        old: hunk_old,
                        new: hunk_new,
                        heading: heading.map(str::to_owned),
                        lines: Arc::from([]),
                    });
                }

                if let Some(context) = line.strip_prefix(' ') {
                    lines.push(Line::Context {
                        old,
                        new,
                        text: text(context),
                    });

                    old += 1;
                    new += 1;
                } else if hunk.is_none() && (line.starts_with("--- ") || line.starts_with("+++ ")) {
                    // File headers repeat the paths; the `diff --git`
                    // operands are authoritative. Inside a hunk, a `--- x`
                    // is a deleted line whose text starts with `-- `.
                } else if let Some(added) = line.strip_prefix('+') {
                    lines.push(Line::Added {
                        new,
                        text: text(added),
                    });

                    new += 1;
                    insertions += 1;
                } else if let Some(deleted) = line.strip_prefix('-') {
                    lines.push(Line::Deleted {
                        old,
                        text: text(deleted),
                    });

                    old += 1;
                    deletions += 1;
                }
            }

            close_hunk!();
            push_file!();

            Ok(Self {
                files: Arc::from(files),
            })
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
        // Never inherit stdin: commands like `mktree` read from it and
        // would otherwise block until it closes.
        .stdin(Stdio::null())
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

#[cfg(test)]
mod tests {
    use super::*;
    use pick_test::Directory;
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    fn repo(name: &str) -> Directory {
        Directory::create(
            std::env::temp_dir().join(format!("pick-git-{name}-{}", std::process::id())),
        )
        .unwrap()
    }

    fn project(dir: &Directory) -> Project {
        Project::new(dir.as_ref(), None)
    }

    /// Runs git in the repo, asserting success.
    fn git(dir: &Directory, args: &[&str]) -> String {
        let output = std::process::Command::new("git")
            .current_dir(dir)
            .args(["-c", "user.name=test", "-c", "user.email=test@test"])
            .args(args)
            .output()
            .expect("git runs");

        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    fn write(dir: &Directory, path: &str, contents: &str) {
        fs::write(dir.join(path), contents).unwrap();
    }

    fn commit(dir: &Directory, message: &str) {
        git(dir, &["add", "-A"]);
        git(dir, &["commit", "-q", "-m", message]);
    }

    fn file_at<'a>(files: &'a [File], path: &str) -> &'a File {
        files
            .iter()
            .find(|file| file.path == path)
            .unwrap_or_else(|| panic!("no {path:?} in the diff: {files:#?}"))
    }

    #[tokio::test]
    async fn a_clean_repo_is_an_empty_diff() {
        let dir = repo("clean");
        git(&dir, &["init", "-q"]);
        write(&dir, "f.txt", "base\n");
        commit(&dir, "base");

        let project = project(&dir);
        let status = Status::current(&project).await.unwrap();
        assert!(matches!(status.branch, Branch::Named(_)));

        let diff = Diff::current(&project, &status).await.unwrap();
        assert!(diff.files.is_empty());
    }

    #[tokio::test]
    async fn an_unborn_repo_diffs_against_the_empty_tree() {
        let dir = repo("unborn");
        git(&dir, &["init", "-q"]);

        write(&dir, "staged.txt", "one\ntwo\n");
        git(&dir, &["add", "staged.txt"]);
        write(&dir, "loose.txt", "untracked\n");

        let project = project(&dir);
        let status = Status::current(&project).await.unwrap();
        assert!(matches!(status.branch, Branch::Unborn(_)));
        assert_eq!(status.untracked.len(), 1);
        assert_eq!(status.untracked[0], "loose.txt");

        // Only the staged file is in the diff; `loose.txt` is
        // untracked and `Status`'s job, not the diff's.
        let diff = Diff::current(&project, &status).await.unwrap();
        assert_eq!(diff.files.len(), 1);

        let file = &diff.files[0];
        assert_eq!(file.path, "staged.txt");
        assert_eq!(file.state, State::Created);
        assert_eq!(file.insertions, 2);
        assert_eq!(file.deletions, 0);
        assert_eq!(file.hunks.len(), 1);
        assert_eq!(file.hunks[0].old, Range { start: 0, count: 0 });
        assert_eq!(file.hunks[0].new, Range { start: 1, count: 2 });
    }

    #[tokio::test]
    async fn the_parser_walks_every_case_in_the_patch() {
        let dir = repo("suite");
        git(&dir, &["init", "-q"]);

        write(
            &dir,
            "a.txt",
            "l1\nl2\n\nl4\nl5\nl6\nl7\nl8\nl9\nl10\nl11\nl12\n",
        );
        write(
            &dir,
            "rs.rs",
            "fn alpha() -> i32 {\n    1\n}\n\nfn main() {\n    let x = 1;\n    println!(\"{}\", x);\n}\n",
        );
        write(&dir, "new_base.txt", "hello\n");
        fs::write(dir.join("bin.txt"), "bin\0ary").unwrap();
        write(&dir, "mode.txt", "mode\n");
        write(&dir, "mode2.txt", "mode2\n");
        write(&dir, "nn.txt", "no_newline");
        write(&dir, "crlf.txt", "crlf1\r\ncrlf2\r\n");
        write(&dir, "héllo.txt", "one\n");
        write(&dir, "héllo2.txt", "two\n");
        write(&dir, "space name.txt", "space\n");
        fs::create_dir(dir.join("a")).unwrap();
        write(&dir, "a/x", "inner\n");
        write(&dir, "empties.txt", "before\n\nafter\n");
        write(&dir, "add_empty.txt", "one\ntwo\n");

        commit(&dir, "base");

        write(
            &dir,
            "a.txt",
            "l1\nchanged2\n\nl4\nl5\nl6\nl7\nl8\nl9\nl10-edited\nl11\nl12\n",
        );
        write(
            &dir,
            "rs.rs",
            "fn alpha() -> i32 {\n    1\n}\n\nfn main() {\n    let x = 2;\n    println!(\"{}\", x);\n}\n",
        );
        write(&dir, "new.txt", "world\n");
        git(&dir, &["add", "new.txt"]);
        git(&dir, &["rm", "-q", "new_base.txt"]);
        git(&dir, &["mv", "héllo.txt", "héllo_renamed.txt"]);
        git(&dir, &["mv", "héllo2.txt", "héllo2_renamed.txt"]);
        write(&dir, "héllo2_renamed.txt", "two\nmore\n");
        fs::write(dir.join("bin.txt"), "bin\0ary2").unwrap();
        // Windows file systems do not track the executable bit, so
        // the mode changes below are Unix-only.
        #[cfg(unix)]
        {
            fs::set_permissions(dir.join("mode.txt"), fs::Permissions::from_mode(0o755)).unwrap();
            fs::set_permissions(dir.join("mode2.txt"), fs::Permissions::from_mode(0o755)).unwrap();
        }
        write(&dir, "mode2.txt", "mode2\nedit\n");
        write(&dir, "nn.txt", "no_newline_changed");
        write(&dir, "crlf.txt", "crlf1x\r\ncrlf2\r\n");
        write(&dir, "space name.txt", "space\nspaced\n");
        write(&dir, "a/x", "inner\nnested\n");
        write(&dir, "empties.txt", "before\nafter\n");
        write(&dir, "add_empty.txt", "one\n\ntwo\n");

        let project = project(&dir);
        let status = Status::current(&project).await.unwrap();
        assert!(matches!(status.branch, Branch::Named(_)));

        // The numstat skips the binary file and sums the
        // low-similarity "rename" as a delete plus a create.
        assert_eq!(status.additions, 12);
        assert_eq!(status.deletions, 8);

        let diff = Diff::current(&project, &status).await.unwrap();

        // `mode.txt` surfaces as a mode-only entry, which is
        // invisible on Windows, where modes are not tracked.
        #[cfg(unix)]
        let file_count = 16;
        #[cfg(windows)]
        let file_count = 15;
        assert_eq!(diff.files.len(), file_count);

        // A modified file with two hunks; the second carries the
        // preceding line as its heading.
        let file = file_at(&diff.files, "a.txt");
        assert_eq!(file.state, State::Modified);
        assert_eq!(file.insertions, 2);
        assert_eq!(file.deletions, 2);
        assert_eq!(file.hunks.len(), 2);

        assert_eq!(file.hunks[0].old, Range { start: 1, count: 5 });
        assert_eq!(file.hunks[0].new, Range { start: 1, count: 5 });
        assert_eq!(file.hunks[0].heading, None);
        assert_eq!(
            file.hunks[0].lines.as_ref(),
            &[
                Line::Context {
                    old: 1,
                    new: 1,
                    text: "l1".into()
                },
                Line::Deleted {
                    old: 2,
                    text: "l2".into()
                },
                Line::Added {
                    new: 2,
                    text: "changed2".into()
                },
                // The empty source line is a single-space context line.
                Line::Context {
                    old: 3,
                    new: 3,
                    text: String::new()
                },
                Line::Context {
                    old: 4,
                    new: 4,
                    text: "l4".into()
                },
                Line::Context {
                    old: 5,
                    new: 5,
                    text: "l5".into()
                },
            ]
        );

        assert_eq!(file.hunks[1].old, Range { start: 7, count: 6 });
        assert_eq!(file.hunks[1].new, Range { start: 7, count: 6 });
        assert_eq!(file.hunks[1].heading.as_deref(), Some("l6"));
        assert_eq!(
            file.hunks[1].lines[3..],
            [
                Line::Deleted {
                    old: 10,
                    text: "l10".into()
                },
                Line::Added {
                    new: 10,
                    text: "l10-edited".into()
                },
                Line::Context {
                    old: 11,
                    new: 11,
                    text: "l11".into()
                },
                Line::Context {
                    old: 12,
                    new: 12,
                    text: "l12".into()
                },
            ]
        );

        // A function-context heading.
        let file = file_at(&diff.files, "rs.rs");
        assert_eq!(
            file.hunks[0].heading.as_deref(),
            Some("fn alpha() -> i32 {")
        );
        assert_eq!(
            file.hunks[0].lines[3],
            Line::Deleted {
                old: 6,
                text: "    let x = 1;".into()
            }
        );
        assert_eq!(
            file.hunks[0].lines[4],
            Line::Added {
                new: 6,
                text: "    let x = 2;".into()
            }
        );

        // A staged new file.
        let file = file_at(&diff.files, "new.txt");
        assert_eq!(file.state, State::Created);
        assert_eq!(file.hunks[0].old, Range { start: 0, count: 0 });
        assert_eq!(file.hunks[0].new, Range { start: 1, count: 1 });
        assert_eq!(
            file.hunks[0].lines.as_ref(),
            &[Line::Added {
                new: 1,
                text: "world".into()
            }]
        );

        // A staged deletion.
        let file = file_at(&diff.files, "new_base.txt");
        assert_eq!(file.state, State::Deleted);
        assert_eq!(file.hunks[0].new, Range { start: 0, count: 0 });
        assert_eq!(
            file.hunks[0].lines.as_ref(),
            &[Line::Deleted {
                old: 1,
                text: "hello".into()
            }]
        );

        // A rename, detected: the new name is the path, the old the
        // original.
        let file = file_at(&diff.files, "héllo_renamed.txt");
        assert_eq!(file.original.as_deref(), Some("héllo.txt"));
        assert_eq!(file.state, State::Modified);
        assert!(file.hunks.is_empty());

        // A rename below the similarity threshold surfaces as a
        // delete plus a create.
        let file = file_at(&diff.files, "héllo2.txt");
        assert_eq!(file.state, State::Deleted);
        assert!(file.original.is_none());
        let file = file_at(&diff.files, "héllo2_renamed.txt");
        assert_eq!(file.state, State::Created);
        assert_eq!(file.insertions, 2);
        assert!(file.original.is_none());

        // A binary file: a marker, not numbers.
        let file = file_at(&diff.files, "bin.txt");
        assert_eq!(file.state, State::Binary);
        assert!(file.hunks.is_empty());
        assert_eq!((file.insertions, file.deletions), (0, 0));

        // A mode-only change; invisible on Windows, so
        // `mode.txt` is not in the diff there at all.
        #[cfg(unix)]
        {
            let file = file_at(&diff.files, "mode.txt");
            assert_eq!(file.state, State::ModeChanged);
            assert!(file.mode_changed);
            assert!(file.hunks.is_empty());
        }

        // A content change with, on Unix, a mode change as well:
        // both are recorded.
        let file = file_at(&diff.files, "mode2.txt");
        assert_eq!(file.state, State::Modified);
        #[cfg(unix)]
        assert!(file.mode_changed);
        assert_eq!(file.insertions, 1);
        assert_eq!(
            file.hunks[0].lines.as_ref(),
            &[
                Line::Context {
                    old: 1,
                    new: 1,
                    text: "mode2".into()
                },
                Line::Added {
                    new: 2,
                    text: "edit".into()
                },
            ]
        );

        // No trailing newline on either side: the markers are skipped.
        let file = file_at(&diff.files, "nn.txt");
        assert_eq!(
            file.hunks[0].lines.as_ref(),
            &[
                Line::Deleted {
                    old: 1,
                    text: "no_newline".into()
                },
                Line::Added {
                    new: 1,
                    text: "no_newline_changed".into()
                },
            ]
        );

        // CRLF files: the `\r` is not carried into the text.
        let file = file_at(&diff.files, "crlf.txt");
        assert_eq!(
            file.hunks[0].lines.as_ref(),
            &[
                Line::Deleted {
                    old: 1,
                    text: "crlf1".into()
                },
                Line::Added {
                    new: 1,
                    text: "crlf1x".into()
                },
                Line::Context {
                    old: 2,
                    new: 2,
                    text: "crlf2".into()
                },
            ]
        );

        // Spaces in paths are raw; split on the last ` b/`.
        // (The change appends a line, so there are no deletions.)
        let file = file_at(&diff.files, "space name.txt");
        assert_eq!(file.insertions, 1);
        assert_eq!(file.deletions, 0);

        // A file in a directory named `a`: the prefix is stripped once.
        let file = file_at(&diff.files, "a/x");
        assert_eq!(file.insertions, 1);

        // Deleting an empty line: a bare `-`, and the numbers shift.
        let file = file_at(&diff.files, "empties.txt");
        assert_eq!(
            file.hunks[0].lines.as_ref(),
            &[
                Line::Context {
                    old: 1,
                    new: 1,
                    text: "before".into()
                },
                Line::Deleted {
                    old: 2,
                    text: String::new()
                },
                Line::Context {
                    old: 3,
                    new: 2,
                    text: "after".into()
                },
            ]
        );

        // Adding an empty line: a bare `+`, and the numbers shift.
        let file = file_at(&diff.files, "add_empty.txt");
        assert_eq!(
            file.hunks[0].lines.as_ref(),
            &[
                Line::Context {
                    old: 1,
                    new: 1,
                    text: "one".into()
                },
                Line::Added {
                    new: 2,
                    text: String::new()
                },
                Line::Context {
                    old: 2,
                    new: 3,
                    text: "two".into()
                },
            ]
        );
    }

    #[tokio::test]
    async fn conflict_markers_parse_as_ordinary_added_lines() {
        let dir = repo("conflict");
        git(&dir, &["init", "-q"]);
        write(&dir, "f.txt", "base\n");
        commit(&dir, "base");

        git(&dir, &["checkout", "-qb", "feature"]);
        write(&dir, "f.txt", "feat\n");
        commit(&dir, "feature");

        git(&dir, &["checkout", "-q", "-"]);
        write(&dir, "f.txt", "main\n");
        commit(&dir, "main");

        // The merge conflicts; the failure is expected.
        let output = std::process::Command::new("git")
            .current_dir(&dir)
            .args([
                "-c",
                "user.name=test",
                "-c",
                "user.email=test@test",
                "merge",
                "--no-commit",
                "--no-ff",
                "feature",
            ])
            .output()
            .expect("git runs");
        // The conflict report goes to stdout, not stderr.
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains("CONFLICT"));

        let project = project(&dir);
        let status = Status::current(&project).await.unwrap();
        assert!(matches!(status.branch, Branch::Named(_)));

        let diff = Diff::current(&project, &status).await.unwrap();
        let file = file_at(&diff.files, "f.txt");
        assert_eq!(file.insertions, 4);
        assert_eq!(
            file.hunks[0].lines.as_ref(),
            &[
                Line::Added {
                    new: 1,
                    text: "<<<<<<< HEAD".into()
                },
                Line::Context {
                    old: 1,
                    new: 2,
                    text: "main".into()
                },
                Line::Added {
                    new: 3,
                    text: "=======".into()
                },
                Line::Added {
                    new: 4,
                    text: "feat".into()
                },
                Line::Added {
                    new: 5,
                    text: ">>>>>>> feature".into()
                },
            ]
        );
    }
}
