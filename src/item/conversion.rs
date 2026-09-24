use crate::core::session;
use crate::diff;
use crate::item::{Compaction, Reply, Status, ToolRun};
use crate::markdown::Markdown;
use crate::repository;
use crate::tool;
use crate::{Item, Tool};

use std::collections::BTreeMap;
use std::sync::Arc;

impl Item {
    pub fn from_session(tools: &BTreeMap<&str, Tool>, item: session::Item) -> Self {
        match item {
            session::Item::User(content) => Self::User(Markdown::new(&content)),
            session::Item::Assistant(reply) => Self::Assistant(Reply::from_session(reply)),
            session::Item::Tool(tool_run) => Self::Tool(ToolRun::from_session(tools, tool_run)),
            session::Item::Compaction(compaction) => {
                Self::Compaction(Compaction::from_session(compaction))
            }
            session::Item::Review(review) => Self::Review(repository::Review::from_session(review)),
        }
    }

    pub fn to_session(&self) -> session::Item {
        match self {
            Self::User(markdown) => session::Item::User(markdown.raw().to_owned()),
            Self::Assistant(reply) => session::Item::Assistant(reply.to_session()),
            Self::Tool(tool_run) => session::Item::Tool(tool_run.to_session()),
            Self::Compaction(compaction) => session::Item::Compaction(compaction.to_session()),
            Self::Review(review) => session::Item::Review(review.to_session()),
        }
    }
}

impl Reply {
    pub fn from_session(reply: session::Reply) -> Self {
        Self {
            prompt: reply.prompt,
            reasoning: Markdown::new(&reply.reasoning),
            content: Markdown::new(&reply.content),
            tool_calls: reply.tool_calls,
            timings: reply.timings,
        }
    }

    pub fn to_session(&self) -> session::Reply {
        session::Reply {
            prompt: self.prompt,
            reasoning: self.reasoning.raw().to_owned(),
            content: self.content.raw().to_owned(),
            tool_calls: self.tool_calls.clone(),
            timings: self.timings,
        }
    }
}

impl ToolRun {
    pub fn from_session(tools: &BTreeMap<&str, Tool>, tool_run: session::ToolRun) -> Self {
        let state = match tools.get(tool_run.call.name.as_str()) {
            Some(tool) => tool.parse(&tool_run.call.arguments),
            None => Err(std::io::Error::other(format!(
                "unknown tool: {name}",
                name = tool_run.call.name
            ))
            .into()),
        };

        Self {
            call: tool_run.call,
            state,
            status: Status::from_session(tool_run.status),
        }
    }

    pub fn to_session(&self) -> session::ToolRun {
        session::ToolRun {
            call: self.call.clone(),
            status: self.status.to_session(),
        }
    }
}

impl Status {
    pub fn from_session(status: session::Status) -> Self {
        match status {
            session::Status::Success { output } => Self::Success { output },
            session::Status::Error { output } => Self::Error { output },
            session::Status::Invalid => Self::Invalid,
            session::Status::Aborted => Self::Aborted,
        }
    }

    pub fn to_session(&self) -> session::Status {
        match self {
            Self::Success { output } => session::Status::Success {
                output: output.clone(),
            },
            Self::Error { output } => session::Status::Error {
                output: output.clone(),
            },
            Self::Invalid => session::Status::Invalid,
            Self::Aborted => session::Status::Aborted,
            Self::Running { .. } => {
                log::warn!("running tool is being saved!");
                session::Status::Aborted
            }
        }
    }
}

impl Compaction {
    pub fn from_session(compaction: session::Compaction) -> Self {
        Self {
            reply: Reply::from_session(compaction.reply),
            tokens: compaction.tokens,
            reasoning_tokens: compaction.reasoning_tokens,
            to: compaction.to,
            is_finished: true,
        }
    }

    pub fn to_session(&self) -> session::Compaction {
        session::Compaction {
            reply: self.reply.to_session(),
            tokens: self.tokens,
            reasoning_tokens: self.reasoning_tokens,
            to: self.to,
        }
    }
}

impl repository::Review {
    pub fn from_session(session: session::Review) -> Self {
        let comments: Vec<repository::Comment> = session
            .comments
            .into_iter()
            .map(repository::Comment::from_session)
            .collect();

        Self {
            message: session.message.map(|message| Markdown::new(&message)),
            comments: Arc::from(comments),
        }
    }

    pub fn to_session(&self) -> session::Review {
        session::Review {
            message: self
                .message
                .as_ref()
                .map(|markdown| markdown.raw().to_owned()),
            comments: self
                .comments
                .iter()
                .map(repository::Comment::to_session)
                .collect(),
        }
    }
}

impl repository::Comment {
    pub fn from_session(session: session::Comment) -> Self {
        let index = diff::Index {
            path: session.path.into(),
            number: session.number,
        };

        let hunk = diff::Hunk::from_git(&index.path, &session.hunk, tool::BACKGROUND);

        Self {
            hunk,
            index,
            content: Markdown::new(&session.content),
        }
    }

    pub fn to_session(&self) -> session::Comment {
        session::Comment {
            path: self.index.path.to_string(),
            number: self.index.number,
            hunk: self.hunk.raw().clone(),
            content: self.content.raw().to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::core::git;
    use iced::Color;

    #[test]
    fn a_review_roundtrips_through_a_session() {
        let first_hunk = git::Hunk {
            old: git::Range {
                start: 122,
                count: 2,
            },
            new: git::Range {
                start: 122,
                count: 2,
            },
            heading: Some("@@ -122,2 +122,2 @@ fn main".to_owned()),
            lines: Arc::from([
                git::Line::Context {
                    old: 122,
                    new: 122,
                    text: "let x = 1;".to_owned(),
                },
                git::Line::Added {
                    new: 123,
                    text: "let y = 2;".to_owned(),
                },
            ]),
        };

        let second_hunk = git::Hunk {
            old: git::Range { start: 7, count: 1 },
            new: git::Range { start: 6, count: 0 },
            heading: None,
            lines: Arc::from([git::Line::Deleted {
                old: 7,
                text: "obsolete".to_owned(),
            }]),
        };

        let review = repository::Review {
            message: Some(Markdown::new("fix these")),
            comments: Arc::from([
                repository::Comment {
                    hunk: diff::Hunk::from_git("src/lib.rs", &first_hunk, Color::TRANSPARENT),
                    index: diff::Index {
                        path: "src/lib.rs".into(),
                        number: git::Number::New(123),
                    },
                    content: Markdown::new("check this"),
                },
                repository::Comment {
                    hunk: diff::Hunk::from_git("src/main.rs", &second_hunk, Color::TRANSPARENT),
                    index: diff::Index {
                        path: "src/main.rs".into(),
                        number: git::Number::Old(7),
                    },
                    content: Markdown::new("drop this"),
                },
            ]),
        };

        let review = repository::Review::from_session(review.to_session());

        assert_eq!(review.message.unwrap().raw(), "fix these");

        let [first, second] = review.comments.as_ref() else {
            unreachable!()
        };

        assert_eq!(first.index.path.as_ref(), "src/lib.rs");
        assert_eq!(first.index.number, git::Number::New(123));
        assert_eq!(first.content.raw(), "check this");

        // The hunk is recreated from the persisted lines
        assert_eq!(first.hunk.raw(), &first_hunk);

        assert_eq!(second.index.path.as_ref(), "src/main.rs");
        assert_eq!(second.index.number, git::Number::Old(7));
        assert_eq!(second.content.raw(), "drop this");

        assert_eq!(second.hunk.raw(), &second_hunk);
    }
}
