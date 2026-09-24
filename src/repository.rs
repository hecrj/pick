use crate::core::Project;
use crate::core::git;
use crate::diff;
use crate::font;
use crate::markdown::{self, Markdown};
use crate::tool;

use iced::border;
use iced::keyboard;
use iced::padding;
use iced::widget::{
    button, center, column, container, rich_text, right, row, span, sticky, text, text_editor,
};
use iced::{Bottom, Center, Element, Fill, Fit, Font, Task, Theme, never};

use std::collections::HashMap;
use std::sync::Arc;

pub struct Repository {
    project: Project,
    pub status: Option<git::Status>,
    files: Vec<File>,
    comments: HashMap<diff::Index, Draft>,
    review: text_editor::Content,
}

#[derive(Debug, Clone)]
pub enum Message {
    Changed(git::Result<git::Status>),
    Diffed(git::Result<Vec<File>>),
    Comment(diff::Index),
    CommentChanged(diff::Index, text_editor::Action),
    CancelComment(diff::Index),
    SendComment(diff::Index),
    AddToReview(diff::Index),
    ReviewChanged(text_editor::Action),
    SendReview,
    LinkClicked(markdown::Uri),
}

#[derive(Debug)]
pub enum Action {
    None,
    Run(Task<Message>),
    Review(Review),
}

impl Repository {
    pub fn new(project: Project) -> (Self, Task<Message>) {
        let repository = Self {
            project,
            status: None,
            files: Vec::new(),
            comments: HashMap::new(),
            review: text_editor::Content::new(),
        };

        let status = repository.status();

        (repository, status)
    }

    pub fn update(&mut self, message: Message) -> Action {
        match message {
            Message::Changed(status) => {
                self.status = status.ok();

                if self.files.is_empty() {
                    Action::Run(self.diff())
                } else {
                    Action::None
                }
            }
            Message::Diffed(Ok(files)) => {
                self.files = files;

                Action::None
            }
            Message::Diffed(Err(error)) => {
                log::error!("{error}");

                Action::None
            }
            Message::Comment(index) => {
                self.comments
                    .insert(index, Draft::Writing(text_editor::Content::new()));

                Action::None
            }
            Message::CommentChanged(index, action) => {
                let Some(Draft::Writing(content)) = self.comments.get_mut(&index) else {
                    return Action::None;
                };

                content.perform(action);

                Action::None
            }
            Message::CancelComment(index) => {
                let _ = self.comments.remove(&index);

                Action::None
            }
            Message::SendComment(index) => {
                let Some(comment) = self.comments.get_mut(&index) else {
                    return Action::None;
                };

                let Draft::Writing(content) = comment else {
                    return Action::None;
                };

                *comment = Draft::Pending(Markdown::new(content.text().trim()));

                let Some(review) = self.finish_review() else {
                    return Action::None;
                };

                Action::Review(review)
            }
            Message::AddToReview(index) => {
                let Some(comment) = self.comments.get_mut(&index) else {
                    return Action::None;
                };

                let Draft::Writing(content) = comment else {
                    return Action::None;
                };

                *comment = Draft::Pending(Markdown::new(content.text().trim()));

                Action::None
            }
            Message::ReviewChanged(action) => {
                self.review.perform(action);

                Action::None
            }
            Message::SendReview => {
                let Some(review) = self.finish_review() else {
                    return Action::None;
                };

                Action::Review(review)
            }
            Message::LinkClicked(uri) => {
                dbg!(uri); // TODO

                Action::None
            }
        }
    }

    pub fn status(&self) -> Task<Message> {
        Task::perform(git::Status::current(&self.project), Message::Changed)
    }

    pub fn diff(&self) -> Task<Message> {
        let Some(status) = &self.status else {
            return Task::none();
        };

        let project = self.project.clone();
        let status = status.clone();

        Task::perform(
            async move {
                let diff = git::Diff::current(&project, &status).await?;

                let tasks: Vec<Vec<_>> = diff
                    .files
                    .iter()
                    .map(|file| {
                        file.hunks
                            .iter()
                            .map(|hunk| {
                                let path = file.path.to_owned();
                                let hunk = hunk.clone();

                                tokio::task::spawn_blocking(move || {
                                    diff::Hunk::from_git(
                                        &path,
                                        &hunk,
                                        Theme::CatppuccinMocha.seed().background,
                                    )
                                })
                            })
                            .collect()
                    })
                    .collect();

                let mut files = Vec::with_capacity(diff.files.len());

                for (file, tasks) in diff.files.iter().zip(tasks) {
                    let mut hunks = Vec::new();

                    for (hunk, task) in file.hunks.iter().zip(tasks) {
                        hunks.push(Hunk {
                            raw: hunk.clone(),
                            diff: task.await?,
                        });
                    }

                    files.push(File {
                        raw: file.clone(),
                        hunks: Arc::from(hunks),
                    });
                }

                Ok(files)
            },
            Message::Diffed,
        )
    }

    pub fn is_reviewing(&self) -> bool {
        self.comments
            .values()
            .any(|comment| matches!(comment, Draft::Pending(_)))
    }

    pub fn finish_review(&mut self) -> Option<Review> {
        let mut comments = Vec::new();
        let mut pending = Vec::new();

        for (index, comment) in self.comments.drain() {
            let Draft::Pending(content) = comment else {
                pending.push((index, comment));
                continue;
            };

            // The file, hunk, and line the comment resolves to, in
            // the view it was made in
            let found = self
                .files
                .iter()
                .enumerate()
                .find_map(|(file_index, file)| {
                    if index.path.as_ref() != file.raw.path.as_str() {
                        return None;
                    }

                    file.hunks
                        .iter()
                        .enumerate()
                        .find_map(|(hunk_index, hunk)| {
                            hunk.diff
                                .position(&index)
                                .map(|position| (file_index, hunk_index, hunk, position))
                        })
                });

            let Some((file_index, hunk_index, hunk, position)) = found else {
                log::debug!("comment on {} no longer resolves to a line", index.path);
                continue;
            };

            // The commented line and the two lines before it,
            // without their superfluous indentation
            let start = position.saturating_sub(2);
            let slice = hunk.raw.slice(start..=position).trim();
            let hunk = diff::Hunk::from_git(&index.path, &slice, tool::BACKGROUND);

            // With the order of the line in the diff: file, hunk, line
            comments.push((
                (file_index, hunk_index, position),
                Comment {
                    index,
                    hunk,
                    content,
                },
            ));
        }

        self.comments = HashMap::from_iter(pending);

        if comments.is_empty() {
            return None;
        }

        // The order the diff presents its lines, as the anchors' own
        // order groups by kind, not by line
        comments.sort_by_key(|(order, _)| *order);

        let comments = comments
            .into_iter()
            .map(|(_, comment)| comment)
            .collect::<Vec<_>>();

        let text = self.review.text();
        let message = text.trim();
        let message = (!message.is_empty()).then(|| Markdown::new(message));

        // The review's message is consumed with it
        self.review = text_editor::Content::new();

        Some(Review {
            message,
            comments: Arc::from(comments),
        })
    }

    /// The review's message input, while a review is being written
    pub fn message(&self) -> Option<Element<'_, Message>> {
        if !self.is_reviewing() {
            return None;
        }

        let editor = text_editor(&self.review)
            .id("review")
            .placeholder("Leave a review")
            .padding(10)
            .on_action(Message::ReviewChanged)
            .key_binding(|key_press| {
                if key_press.is_focused
                    && key_press.key == keyboard::Key::Named(keyboard::key::Named::Enter)
                    && !key_press.modifiers.shift()
                {
                    return Some(text_editor::Binding::Custom(Message::SendReview));
                }

                text_editor::Binding::from_key_press(key_press)
            });

        let control = button(text("Send").font(Font::DEFAULT))
            .padding(10)
            .style(button::success)
            .on_press(Message::SendReview);

        Some(
            row![container(editor).width(Fill), control]
                .spacing(10)
                .align_y(Bottom)
                .into(),
        )
    }

    pub fn review(&self) -> Element<'_, Message> {
        if self.files.is_empty() {
            return center("Working tree is clean!").into();
        }

        column(self.files.iter().map(|file| {
            let header = container(
                row![
                    text(&file.raw.path)
                        .size(font::SMALL)
                        .width(Fill)
                        .wrapping(text::Wrapping::None)
                        .ellipsis(text::Ellipsis::End)
                        .line_height(1.5),
                    text!("+{}", file.raw.insertions)
                        .style(text::success)
                        .size(font::SMALL),
                    text!("-{}", file.raw.deletions)
                        .style(text::danger)
                        .size(font::SMALL),
                ]
                .spacing(10)
                .align_y(Center),
            )
            .padding(11)
            .style(|theme| {
                container::Style::default()
                    .background(theme.palette().background.weakest.color)
                    .border(
                        border::rounded(border::top(5))
                            .width(1)
                            .color(theme.palette().background.weak.color),
                    )
            });

            let is_reviewing = self.is_reviewing();

            let hunks = column(
                file.hunks
                    .iter()
                    .map(|hunk| hunk.view(&self.comments, is_reviewing)),
            )
            .spacing(10);

            container(column![
                sticky(container(header).padding(padding::top(10)).style(|theme| {
                    container::Style::default().background(theme.seed().background)
                })),
                hunks.padding(padding::horizontal(1)),
            ])
            .style(|theme| container::Style {
                border: border::rounded(border::bottom(5))
                    .width(1)
                    .color(theme.palette().background.weak.color),
                ..container::Style::default()
            })
            .into()
        }))
        .spacing(10)
        .height(Fill)
        .into()
    }
}

#[derive(Debug, Clone)]
pub struct File {
    pub raw: git::File,
    pub hunks: Arc<[Hunk]>,
}

#[derive(Debug, Clone)]
pub struct Hunk {
    raw: git::Hunk,
    diff: diff::Hunk,
}

impl Hunk {
    pub fn view<'a>(
        &'a self,
        comments: &'a HashMap<diff::Index, Draft>,
        is_reviewing: bool,
    ) -> Element<'a, Message> {
        let heading = self
            .diff
            .gutter(format!(
                "  @@ -{} +{} @@{}",
                self.raw.old,
                self.raw.new,
                if let Some(heading) = &self.raw.heading {
                    format!(" {heading}")
                } else {
                    "".to_owned()
                }
            ))
            .map(never);

        column![
            heading,
            column(self.diff.view().map(|view| {
                if let Some(comment) = comments.get(view.index) {
                    comment.view(view, is_reviewing)
                } else {
                    view.with_action(Message::Comment)
                }
            }))
        ]
        .into()
    }
}

#[derive(Debug)]
pub enum Draft {
    Writing(text_editor::Content),
    Pending(Markdown),
}

impl Draft {
    fn view<'a>(&'a self, view: diff::View<'a>, is_reviewing: bool) -> Element<'a, Message> {
        let comment = match self {
            Draft::Writing(content) => {
                let comment = text_editor(content)
                    .placeholder("Leave a comment")
                    .padding(10)
                    .height(Fit.min(80))
                    .on_action(|action| Message::CommentChanged(view.index.clone(), action));

                let controls = {
                    let control = |label| {
                        button(
                            text(label)
                                .font(Font::DEFAULT)
                                .size(font::SMALL)
                                .line_height(1.5),
                        )
                    };

                    row![
                        text!("Add a comment on line {}", view.index.number)
                            .font(font::BOLD)
                            .size(font::SMALL),
                        right(
                            row![
                                control("Cancel")
                                    .style(button::secondary)
                                    .on_press_with(|| Message::CancelComment(view.index.clone())),
                                (!is_reviewing).then(|| control("Send")
                                    .style(button::primary)
                                    .on_press_maybe_with((!content.is_empty()).then_some(|| {
                                        Message::SendComment(view.index.clone())
                                    }))),
                                (!is_reviewing).then(|| control("Start a review")
                                    .style(button::success)
                                    .on_press_maybe_with((!content.is_empty()).then_some(|| {
                                        Message::AddToReview(view.index.clone())
                                    }))),
                                is_reviewing.then(|| control("Add to review")
                                    .style(button::primary)
                                    .on_press_maybe_with((!content.is_empty()).then_some(
                                        move || { Message::AddToReview(view.index.clone()) }
                                    ))),
                            ]
                            .spacing(10)
                        )
                    ]
                    .align_y(Center)
                };

                column![comment, controls].spacing(10)
            }
            Draft::Pending(content) => column![
                rich_text![
                    "Comment on line ",
                    span(view.index.number.to_string()).font(font::BOLD)
                ]
                .on_link_click(never)
                .size(font::TINY)
                .style(text::secondary),
                markdown::view(content.items(), Font::DEFAULT, font::NORMAL)
                    .map(Message::LinkClicked)
            ]
            .spacing(5),
        };

        view.with_decoration(
            container(comment)
                .width(Fill)
                .padding(10)
                .style(container::bordered_box),
        )
    }
}

#[derive(Debug, Default)]
pub struct Review {
    pub message: Option<Markdown>,
    pub comments: Arc<[Comment]>,
}

#[derive(Debug)]
pub struct Comment {
    pub hunk: diff::Hunk,
    pub index: diff::Index,
    pub content: Markdown,
}

impl Review {
    /// The text of the review as sent to the model.
    ///
    /// The message (if any) leads, then one block per comment: the
    /// quoted line, under a `path:R123` reference, in a code block, with
    /// the comment beneath it. The model is expected to have the
    /// surrounding code in context already, so no further context is
    /// quoted.
    pub fn prompt(&self) -> String {
        let mut blocks: Vec<String> = Vec::new();

        if let Some(message) = &self.message {
            let message = message.raw().trim();

            if !message.is_empty() {
                blocks.push(message.to_owned());
            }
        }

        for comment in self.comments.iter() {
            let Some(line) = comment.hunk.last_line() else {
                continue;
            };

            let line = line.text();

            let mut block = vec![
                "```".to_owned(),
                comment.index.to_string(),
                line,
                "```".to_owned(),
            ];

            let content = comment.content.raw().trim();

            if !content.is_empty() {
                block.push(content.to_owned());
            }

            blocks.push(block.join("\n"));
        }

        blocks.join("\n\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::Color;

    /// The file the test hunks live in
    const PATH: &str = "src/lib.rs";

    /// A 20-line hunk with a deletion and an addition in each half
    fn raw_hunk() -> git::Hunk {
        git::Hunk {
            old: git::Range {
                start: 1,
                count: 18,
            },
            new: git::Range {
                start: 1,
                count: 18,
            },
            heading: Some("fn add".into()),
            lines: Arc::from([
                git::Line::Context {
                    old: 1,
                    new: 1,
                    text: "a".into(),
                },
                git::Line::Context {
                    old: 2,
                    new: 2,
                    text: "b".into(),
                },
                git::Line::Deleted {
                    old: 3,
                    text: "c".into(),
                },
                git::Line::Added {
                    new: 3,
                    text: "C".into(),
                },
                git::Line::Context {
                    old: 4,
                    new: 4,
                    text: "d".into(),
                },
                git::Line::Context {
                    old: 5,
                    new: 5,
                    text: "e".into(),
                },
                git::Line::Context {
                    old: 6,
                    new: 6,
                    text: "f".into(),
                },
                git::Line::Context {
                    old: 7,
                    new: 7,
                    text: "g".into(),
                },
                git::Line::Context {
                    old: 8,
                    new: 8,
                    text: "h".into(),
                },
                git::Line::Context {
                    old: 9,
                    new: 9,
                    text: "i".into(),
                },
                git::Line::Context {
                    old: 10,
                    new: 10,
                    text: "j".into(),
                },
                git::Line::Context {
                    old: 11,
                    new: 11,
                    text: "k".into(),
                },
                git::Line::Deleted {
                    old: 12,
                    text: "l".into(),
                },
                git::Line::Context {
                    old: 13,
                    new: 12,
                    text: "m".into(),
                },
                git::Line::Context {
                    old: 14,
                    new: 13,
                    text: "n".into(),
                },
                git::Line::Added {
                    new: 14,
                    text: "N".into(),
                },
                git::Line::Context {
                    old: 15,
                    new: 15,
                    text: "o".into(),
                },
                git::Line::Context {
                    old: 16,
                    new: 16,
                    text: "p".into(),
                },
                git::Line::Context {
                    old: 17,
                    new: 17,
                    text: "q".into(),
                },
                git::Line::Context {
                    old: 18,
                    new: 18,
                    text: "r".into(),
                },
            ]),
        }
    }

    fn file(path: &str) -> File {
        let raw = raw_hunk();
        let diff = diff::Hunk::from_git(path, &raw, tool::BACKGROUND);

        File {
            raw: git::File {
                path: path.to_owned(),
                original: None,
                state: git::State::Modified,
                mode_changed: false,
                insertions: 2,
                deletions: 2,
                hunks: Arc::from([raw.clone()]),
            },
            hunks: Arc::from([Hunk { raw, diff }]),
        }
    }

    fn anchor(path: &str, number: git::Number) -> diff::Index {
        diff::Index {
            path: path.into(),
            number,
        }
    }

    fn pending(index: diff::Index, content: &str) -> (diff::Index, Draft) {
        (index, Draft::Pending(Markdown::new(content)))
    }

    /// The text of a raw line
    fn repository(
        files: Vec<File>,
        comments: Vec<(diff::Index, Draft)>,
        review: &str,
    ) -> Repository {
        Repository {
            project: Project::current_dir().unwrap(),
            status: None,
            files,
            comments: comments.into_iter().collect(),
            review: text_editor::Content::with_text(review),
        }
    }

    #[test]
    fn without_pending_comments_the_review_is_none() {
        let index = anchor(PATH, git::Number::New(2));
        let mut repository = repository(
            vec![file(PATH)],
            vec![(index.clone(), Draft::Writing(text_editor::Content::new()))],
            "a pending message",
        );

        assert!(repository.finish_review().is_none());

        // The comment still being written and the message are
        // left behind
        assert!(matches!(
            repository.comments.get(&index),
            Some(Draft::Writing(_))
        ));
        assert_eq!(repository.review.text(), "a pending message");
    }

    #[test]
    fn comments_keep_their_line_and_the_two_before_it() {
        let mut repository = repository(
            vec![file(PATH)],
            vec![
                pending(anchor(PATH, git::Number::Context(16, 16)), "far away"),
                pending(anchor(PATH, git::Number::Context(2, 2)), "close one"),
                pending(anchor(PATH, git::Number::Context(5, 5)), "close two"),
            ],
            "  review this  ",
        );

        let Some(review) = repository.finish_review() else {
            unreachable!()
        };

        // The comments are sorted by line
        let [first, second, third] = &review.comments[..] else {
            unreachable!()
        };

        assert_eq!(first.index.number, git::Number::Context(2, 2));
        assert_eq!(second.index.number, git::Number::Context(5, 5));
        assert_eq!(third.index.number, git::Number::Context(16, 16));

        // Each keeps its line and the two before it; the first, near the
        // top of the hunk, has only two lines in all
        assert_eq!(first.hunk.lines().len(), 2);
        assert_eq!(second.hunk.lines().len(), 3);
        assert_eq!(third.hunk.lines().len(), 3);

        assert_eq!(first.hunk.last_line().unwrap().text(), "  b");
        assert_eq!(second.hunk.last_line().unwrap().text(), "  e");
        assert_eq!(third.hunk.last_line().unwrap().text(), "  p");

        // The comments keep their content
        assert_eq!(first.content.raw(), "close one");
        assert_eq!(second.content.raw(), "close two");
        assert_eq!(third.content.raw(), "far away");

        // The pending comments and the message are consumed
        assert!(repository.comments.is_empty());
        assert_eq!(review.message.unwrap().raw(), "review this");
        assert!(repository.review.text().is_empty());
    }

    #[test]
    fn a_blank_review_message_is_dropped() {
        let mut repository = repository(
            vec![file(PATH)],
            vec![pending(
                anchor(PATH, git::Number::Context(2, 2)),
                "a comment",
            )],
            "   ",
        );

        let Some(review) = repository.finish_review() else {
            unreachable!()
        };

        assert!(review.message.is_none());
        assert_eq!(review.comments.len(), 1);
    }

    #[test]
    fn comments_on_missing_lines_are_dropped() {
        let mut repository = repository(
            vec![file(PATH)],
            vec![
                pending(anchor(PATH, git::Number::New(99)), "stale"),
                pending(anchor(PATH, git::Number::Old(3)), "the deletion"),
            ],
            "",
        );

        let Some(review) = repository.finish_review() else {
            unreachable!()
        };

        // The comment on the deleted line survives; the stale one
        // is dropped
        let [comment] = &review.comments[..] else {
            unreachable!()
        };

        assert_eq!(comment.index.number, git::Number::Old(3));
        assert_eq!(comment.hunk.last_line().unwrap().text(), "- c");
    }

    #[test]
    fn only_stale_comments_yield_no_review() {
        let mut repository = repository(
            vec![file(PATH)],
            vec![pending(anchor(PATH, git::Number::New(99)), "stale")],
            "",
        );

        assert!(repository.finish_review().is_none());
    }

    #[test]
    fn comments_are_sorted_by_path_and_line() {
        let mut repository = repository(
            vec![file("src/a.rs"), file("src/b.rs")],
            vec![
                pending(anchor("src/b.rs", git::Number::Context(2, 2)), "b2"),
                pending(anchor("src/a.rs", git::Number::Context(16, 16)), "a16"),
                pending(anchor("src/a.rs", git::Number::Context(2, 2)), "a2"),
                pending(anchor("src/b.rs", git::Number::Context(16, 16)), "b16"),
            ],
            "",
        );

        let Some(review) = repository.finish_review() else {
            unreachable!()
        };

        let expected = [
            ("src/a.rs", 2),
            ("src/a.rs", 16),
            ("src/b.rs", 2),
            ("src/b.rs", 16),
        ];

        for (comment, (path, line)) in review.comments.iter().zip(expected) {
            assert_eq!(comment.index.path.as_ref(), path);
            assert_eq!(comment.index.number, git::Number::Context(line, line));
        }
    }

    #[test]
    fn comments_are_sorted_by_their_line_not_their_kind() {
        let mut repository = repository(
            vec![file(PATH)],
            vec![
                pending(anchor(PATH, git::Number::New(14)), "an addition, late"),
                pending(anchor(PATH, git::Number::Old(3)), "a deletion, early"),
                pending(
                    anchor(PATH, git::Number::Context(16, 16)),
                    "a context, late",
                ),
                pending(anchor(PATH, git::Number::New(3)), "an addition, early"),
                pending(anchor(PATH, git::Number::Old(12)), "a deletion, late"),
                pending(anchor(PATH, git::Number::Context(2, 2)), "a context, early"),
            ],
            "",
        );

        let Some(review) = repository.finish_review() else {
            unreachable!()
        };

        // The anchors' own order would group all the deletions
        // before the additions, before the contexts; the lines
        // alternate instead
        let expected = [
            git::Number::Context(2, 2),
            git::Number::Old(3),
            git::Number::New(3),
            git::Number::Old(12),
            git::Number::New(14),
            git::Number::Context(16, 16),
        ];

        for (comment, number) in review.comments.iter().zip(expected) {
            assert_eq!(comment.index.number, number);
        }
    }

    #[test]
    fn a_comment_resolves_against_the_displayed_hunk() {
        // Git and the displayed diff may align a hunk differently.
        // Here git claims a deletion and an addition of the same
        // line, while the display sees only context lines
        let raw = git::Hunk {
            old: git::Range { start: 1, count: 3 },
            new: git::Range { start: 1, count: 3 },
            heading: None,
            lines: Arc::from([
                git::Line::Context {
                    old: 1,
                    new: 1,
                    text: "a".into(),
                },
                git::Line::Deleted {
                    old: 2,
                    text: "a".into(),
                },
                git::Line::Added {
                    new: 2,
                    text: "a".into(),
                },
                git::Line::Context {
                    old: 3,
                    new: 3,
                    text: "a".into(),
                },
            ]),
        };

        let diff = diff::Hunk::from_git(PATH, &raw, tool::BACKGROUND);

        let mut repository = repository(
            vec![File {
                raw: git::File {
                    path: PATH.to_owned(),
                    original: None,
                    state: git::State::Modified,
                    mode_changed: false,
                    insertions: 1,
                    deletions: 1,
                    hunks: Arc::from([raw.clone()]),
                },
                hunks: Arc::from([Hunk { raw, diff }]),
            }],
            vec![pending(
                anchor(PATH, git::Number::Context(2, 2)),
                "a context",
            )],
            "",
        );

        let Some(review) = repository.finish_review() else {
            unreachable!()
        };

        let [comment] = &review.comments[..] else {
            unreachable!()
        };

        assert_eq!(comment.index.number, git::Number::Context(2, 2));
        assert_eq!(comment.content.raw(), "a context");
    }

    fn comment(path: &str, number: git::Number, line: git::Line, content: &str) -> Comment {
        let hunk = diff::Hunk::from_git(
            path,
            &git::Hunk {
                old: git::Range { start: 1, count: 1 },
                new: git::Range { start: 1, count: 1 },
                heading: None,
                lines: Arc::from([line]),
            },
            Color::TRANSPARENT,
        );

        Comment {
            index: diff::Index {
                path: path.into(),
                number,
            },
            hunk,
            content: Markdown::new(content),
        }
    }

    #[test]
    fn a_review_prompt_quotes_each_commented_line() {
        let review = Review {
            message: Some(Markdown::new("Fix these")),
            comments: Arc::from([
                comment(
                    "src/lib.rs",
                    git::Number::Context(10, 12),
                    git::Line::Context {
                        old: 10,
                        new: 12,
                        text: "let y = x;".into(),
                    },
                    "note the typo",
                ),
                comment(
                    "src/lib.rs",
                    git::Number::New(123),
                    git::Line::Added {
                        new: 123,
                        text: "fn main() {".into(),
                    },
                    "Check X.",
                ),
                comment(
                    "src/main.rs",
                    git::Number::Old(45),
                    git::Line::Deleted {
                        old: 45,
                        text: "let dead = 1;".into(),
                    },
                    "remove me",
                ),
            ]),
        };

        let expected = [
            "Fix these",
            "",
            "```",
            "src/lib.rs:R12",
            "  let y = x;",
            "```",
            "note the typo",
            "",
            "```",
            "src/lib.rs:R123",
            "+ fn main() {",
            "```",
            "Check X.",
            "",
            "```",
            "src/main.rs:L45",
            "- let dead = 1;",
            "```",
            "remove me",
        ]
        .join("\n");

        assert_eq!(review.prompt(), expected);
    }

    #[test]
    fn a_review_comment_without_content_quotes_the_line_only() {
        let review = Review {
            message: None,
            comments: Arc::from([comment(
                "src/lib.rs",
                git::Number::New(5),
                git::Line::Added {
                    new: 5,
                    text: "x".into(),
                },
                "",
            )]),
        };

        let expected = ["```", "src/lib.rs:R5", "+ x", "```"].join("\n");

        assert_eq!(review.prompt(), expected);
    }

    #[test]
    fn a_review_without_comments_is_just_the_message() {
        let review = Review {
            message: Some(Markdown::new("hello")),
            comments: Arc::from([]),
        };

        assert_eq!(review.prompt(), "hello");
    }

    #[test]
    fn an_empty_review_has_no_prompt() {
        assert_eq!(Review::default().prompt(), "");
    }
}
