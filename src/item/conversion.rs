use crate::core::session;
use crate::item::{Compaction, Reply, Status, ToolRun};
use crate::markdown::Markdown;
use crate::{Item, Tool};

use std::collections::HashMap;

impl Item {
    pub fn from_session(tools: &HashMap<&str, Tool>, item: session::Item) -> Self {
        match item {
            session::Item::User(content) => Self::User(Markdown::new(content)),
            session::Item::Assistant(reply) => Self::Assistant(Reply::from_session(reply)),
            session::Item::Tool(tool_run) => Self::Tool(ToolRun::from_session(tools, tool_run)),
            session::Item::Compaction(compaction) => {
                Self::Compaction(Compaction::from_session(compaction))
            }
        }
    }

    pub fn to_session(&self) -> session::Item {
        match self {
            Self::User(markdown) => session::Item::User(markdown.raw().to_owned()),
            Self::Assistant(reply) => session::Item::Assistant(reply.to_session()),
            Self::Tool(tool_run) => session::Item::Tool(tool_run.to_session()),
            Self::Compaction(compaction) => session::Item::Compaction(compaction.to_session()),
        }
    }
}

impl Reply {
    pub fn from_session(reply: session::Reply) -> Self {
        Self {
            prompt: reply.prompt,
            reasoning: Markdown::new(reply.reasoning),
            content: Markdown::new(reply.content),
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
    pub fn from_session(tools: &HashMap<&str, Tool>, tool_run: session::ToolRun) -> Self {
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
            snap_to_bottom: false,
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
