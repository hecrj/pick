mod file;
mod highlight;
mod markdown;
mod tool;

use crate::markdown::Markdown;
use crate::tool::Tool;

use iced::border;
use iced::font;
use iced::keyboard;
use iced::padding;
use iced::task;
use iced::time;
use iced::widget::operation;
use iced::widget::{
    bottom, center, center_x, column, container, progress_bar, right, row, scrollable, sensor,
    space, stack, text, text_editor,
};
use iced::{Center, Element, Fill, Fit, Font, Pixels, Size, Subscription, Task, Theme, never};

use function::Binary;
use reason::Reason;
use reason::model;

use std::collections::{BTreeMap, HashMap};
use std::env;
use std::path::{Path, PathBuf};

fn main() -> Result<(), iced::Error> {
    tracing_subscriber::fmt::init();

    let prompt = env::args().nth(1);

    iced::application(
        move || Pick::new(prompt.as_deref()),
        Pick::update,
        Pick::view,
    )
    .title("pick")
    .subscription(Pick::subscription)
    .theme(Theme::CatppuccinMocha)
    .font(Font::MONOSPACE)
    .run()
}

struct Pick {
    connection: Connection,
    models: BTreeMap<model::Id, reason::Model>,
    model: Option<model::Id>,
    messages: Vec<Item>,
    input: text_editor::Content,
    input_height: f32,
    content_width: f32,
    snap_to_bottom: bool,
    tasks: HashMap<Work, task::Handle>,
    tools: HashMap<&'static str, Tool>,
    project: PathBuf,
    home: Option<PathBuf>,
    server: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Work {
    Completion,
    Compaction,
    Tool(reason::tool::Id),
}

enum Item {
    User(Markdown),
    Assistant(Reply),
    Tool(ToolRun),
    Compaction(Compaction),
}

impl Item {
    fn to_message(&self) -> Option<reason::Message> {
        Some(match self {
            Item::User(markdown) => reason::Message::User(markdown.raw().to_owned()),
            Item::Assistant(reply) => reason::Message::Assistant(reason::Reply {
                reasoning: reply.reasoning.raw().to_owned(),
                content: reply.content.raw().to_owned(),
                tool_calls: reply.tool_calls.clone(),
            }),
            Item::Tool(tool) => reason::Message::Tool(reason::tool::Response {
                id: tool.call.id.clone(),
                content: tool.status.content().unwrap_or_default(),
            }),
            Item::Compaction { .. } => None?,
        })
    }

    fn view(&self) -> Element<'_, Message> {
        match self {
            Item::Assistant(reply) => {
                let reasoning = if !reply.reasoning.is_empty() {
                    const MAX_HEIGHT: f32 = SMALL as f32 * 1.5 * 15.0; // 15 lines

                    let is_done = !reply.content.is_empty() || !reply.tool_calls.is_empty();
                    let header = container(
                        match reply.timings {
                            Some(timings) => {
                                if is_done {
                                    text!("Thought for {}", duration(timings.reasoning))
                                } else {
                                    text!("Thinking... ({})", duration(timings.reasoning))
                                }
                            }
                            None => text(if is_done { "Thought" } else { "Thinking..." }),
                        }
                        .size(SMALL)
                        .font(Font {
                            weight: font::Weight::Bold,
                            ..Font::MONOSPACE
                        }),
                    )
                    .width(Fill)
                    .padding(padding::bottom(10))
                    .style(|theme: &Theme| {
                        use iced::Color;
                        use iced::gradient;

                        let palette = theme.palette();

                        container::Style {
                            background: Some(
                                gradient::Linear::new(0)
                                    .add_stop(0.0, Color::TRANSPARENT)
                                    .add_stop(0.4, palette.background.weakest.color)
                                    .into(),
                            ),
                            ..container::Style::default()
                        }
                    });

                    Some(
                        container(stack![
                            scrollable(
                                container(
                                    markdown::view(reply.reasoning.items(), Font::MONOSPACE, SMALL)
                                        .map(Message::LinkClicked)
                                )
                                .padding(padding::top(SMALL as f32 * 1.375 + 10.0)),
                            )
                            .width(Fill)
                            .height(Fit.max(MAX_HEIGHT))
                            .spacing(10)
                            .anchor_bottom(),
                            header,
                        ])
                        .style(|theme| container::Style {
                            text_color: Some(theme.palette().secondary.strong.color),
                            background: Some(theme.palette().background.weakest.color.into()),
                            border: border::rounded(5),
                            ..container::transparent(theme)
                        })
                        .padding(10),
                    )
                } else {
                    None
                };

                column![
                    prompt_progress(reply.prompt),
                    reasoning,
                    (!reply.content.is_empty()).then(|| markdown::view(
                        reply.content.items(),
                        Font::DEFAULT,
                        NORMAL
                    )
                    .map(Message::LinkClicked)),
                ]
                .spacing(10)
                .into()
            }
            Item::User(message) => right(
                container(
                    markdown::view(message.items(), Font::DEFAULT, NORMAL)
                        .map(Message::LinkClicked),
                )
                .padding(10)
                .style(container::rounded_box),
            )
            .into(),
            Item::Tool(tool) => {
                let header = {
                    let label = container(text(&tool.call.name).size(SMALL))
                        .padding([2, 5])
                        .style(container::dark);

                    let title = tool
                        .state
                        .as_ref()
                        .ok()
                        .and_then(|state| Some(text(state.title()?).size(SMALL)));

                    row![label, title].spacing(10).align_y(Center)
                };

                let arguments = match &tool.state {
                    Ok(state) => state.view().map(|state| state.map(never)),
                    Err(error) => Some(text!("{error}").size(SMALL).style(text::danger).into()),
                };

                let output: Option<Element<'_, _>> = match &tool.status {
                    Status::Running { logs } => Some(
                        scrollable(
                            column(logs.iter().map(|line| {
                                text(&line[..line.floor_char_boundary(200)])
                                    .wrapping(text::Wrapping::None)
                                    .ellipsis(text::Ellipsis::End)
                                    .size(SMALL)
                                    .line_height(Pixels(TOOL_LOG_LINE_HEIGHT))
                                    .into()
                            }))
                            .spacing(5),
                        )
                        .id(tool.call.id.as_str().to_owned())
                        .width(Fill)
                        .height(Fit.max(MAX_TOOL_LOG_HEIGHT))
                        .on_scroll(|viewport| {
                            let snap_to_bottom = snap_to_bottom(viewport);

                            (tool.snap_to_bottom != snap_to_bottom).then_some(
                                Message::ToolScrolled(tool.call.id.clone(), snap_to_bottom),
                            )
                        })
                        .spacing(10)
                        .into(),
                    ),
                    Status::Success { output } if output.lines() > 0 => {
                        /// The first and last lines a long finished output
                        /// keeps; the middle is elided
                        const HEAD: usize = 2;
                        const TAIL: usize = 3;

                        fn line<'a>(line: &'a str) -> Element<'a, Message> {
                            text(&line[..line.floor_char_boundary(200)])
                                .wrapping(text::Wrapping::None)
                                .ellipsis(text::Ellipsis::End)
                                .size(SMALL)
                                .into()
                        }

                        let total = output.lines();

                        Some(
                            if total > HEAD + TAIL {
                                let elided = total - HEAD - TAIL;

                                let marker = match elided {
                                    1 => "... 1 line elided".to_owned(),
                                    elided => {
                                        format!("... {} lines elided", thousands(elided as u64))
                                    }
                                };

                                column(
                                    output
                                        .head(HEAD)
                                        .map(line)
                                        .chain(std::iter::once(
                                            text(marker).size(SMALL).style(text::secondary).into(),
                                        ))
                                        .chain(output.tail(TAIL).map(line)),
                                )
                                .spacing(5)
                            } else {
                                column(output.all().map(line))
                            }
                            .spacing(5)
                            .into(),
                        )
                    }
                    Status::Success { .. }
                    | Status::Error { .. }
                    | Status::Invalid
                    | Status::Aborted => tool.status.content().map(|content| {
                        let trimmed = content.trim();

                        text(if trimmed.is_empty() {
                            "[No output]".to_owned()
                        } else {
                            trimmed.to_owned()
                        })
                        .size(SMALL)
                        .into()
                    }),
                };

                let output = output.map(|output| {
                    container(output)
                        .width(Fill)
                        .padding(10)
                        .style(|theme: &Theme| {
                            let palette = theme.seed();

                            let color = match tool.status {
                                Status::Running { .. } => palette.warning,
                                Status::Success { .. } => palette.success.scale_alpha(0.5),
                                Status::Invalid | Status::Aborted | Status::Error { .. } => {
                                    palette.danger
                                }
                            };

                            let mut style = container::dark(theme);
                            style.border = style.border.color(color).width(1);
                            style
                        })
                });

                container(column![header, arguments, output].spacing(10))
                    .width(Fill)
                    .padding(10)
                    .style(container::bordered_box)
                    .into()
            }
            Item::Compaction(compaction) => {
                let notice = center_x(text(if compaction.is_finished {
                    format!("Compacted into {} tokens", compaction.tokens)
                } else if compaction.reply.content.is_empty() {
                    format!("Analyzing... {} tokens", compaction.reasoning_tokens)
                } else {
                    format!("Compacting... {} tokens", compaction.tokens)
                }));

                column![prompt_progress(compaction.reply.prompt), notice]
                    .spacing(10)
                    .into()
            }
        }
    }
}

#[derive(Debug, Default)]
struct Reply {
    prompt: reason::Progress,
    reasoning: Markdown,
    content: Markdown,
    tool_calls: Vec<reason::tool::Call>,
    timings: Option<reason::Timings>,
}

struct ToolRun {
    call: reason::tool::Call,
    state: Result<Box<dyn tool::Call>, reason::Error>,
    status: Status,
    snap_to_bottom: bool,
}

#[derive(Debug)]
enum Status {
    Running { logs: Vec<String> },
    Success { output: tool::Output },
    Error { output: String },
    Invalid,
    Aborted,
}

impl Status {
    fn content(&self) -> Option<String> {
        match self {
            Status::Running { .. } => None, // Never send intermediate progress
            Status::Success { output } => Some(output.to_string()),
            Status::Error { output } => Some(output.clone()),
            Status::Invalid => Some("[invalid tool call]".to_owned()),
            Status::Aborted => Some("[execution aborted]".to_owned()),
        }
    }
}

struct Compaction {
    reply: Reply,
    tokens: u64,
    reasoning_tokens: u64,
    to: usize,
    is_finished: bool,
}

#[derive(Debug, Clone)]
enum Connection {
    Disconnected,
    Connecting,
    Connected(Reason),
}

#[derive(Debug, Clone)]
enum Message {
    Connected(Result<(Reason, Vec<reason::Model>), reason::Error>),
    ModelsListed(Result<Vec<reason::Model>, reason::Error>),
    Reconnect,
    InputChanged(text_editor::Action),
    InputResized(Size),
    ContentResized(Size),
    SnapToBottom(bool),
    Send,
    ReplyProgressed(reason::Event),
    ReplyReceived(Result<reason::Reply, reason::Error>),
    CompactionProgressed(reason::Event),
    CompactionReceived(Result<reason::Reply, reason::Error>),
    LinkClicked(markdown::Uri),
    ToolProgressed(reason::tool::Id, String),
    ToolFinished(reason::tool::Id, Result<tool::Output, reason::Error>),
    ToolScrolled(reason::tool::Id, bool),
    Abort,
}

impl Pick {
    fn new(prompt: Option<&str>) -> (Self, Task<Message>) {
        let mut pick = Self {
            connection: Connection::Disconnected,
            models: BTreeMap::new(),
            model: None,
            messages: Vec::new(),
            input: prompt
                .map(text_editor::Content::with_text)
                .unwrap_or_default(),
            input_height: 0.0,
            content_width: 0.0,
            snap_to_bottom: true,
            tasks: HashMap::new(),
            project: env::current_dir().unwrap_or_default(),
            home: env::home_dir(),
            server: "http://127.0.0.1:9931".to_owned(),
            tools: Tool::builtins(),
        };

        let connect = pick.connect();

        (
            pick,
            Task::batch([
                if prompt.is_some() {
                    connect.chain(Task::done(Message::Send))
                } else {
                    connect
                },
                operation::focus("input"),
            ]),
        )
    }

    fn connect(&mut self) -> Task<Message> {
        self.connection = Connection::Connecting;

        Task::perform(Reason::connect(&self.server), Message::Connected)
    }

    fn list_models(&self) -> Task<Message> {
        let Connection::Connected(reason) = self.connection.clone() else {
            return Task::none();
        };

        Task::perform(
            async move { reason.list_models().await },
            Message::ModelsListed,
        )
    }

    fn update_models(&mut self, models: Vec<reason::Model>) -> Task<Message> {
        self.models = models
            .into_iter()
            .map(|model| (model.id.clone(), model))
            .collect();

        if self
            .model
            .as_ref()
            .is_none_or(|model| !self.models.contains_key(model))
        {
            self.model = self.models.keys().next().cloned();
        }

        Task::none()
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Connected(Ok((reason, models))) => {
                self.connection = Connection::Connected(reason);
                self.update_models(models)
            }
            Message::Reconnect => match &self.connection {
                Connection::Disconnected => self.connect(),
                Connection::Connected(_) => self.list_models(),
                Connection::Connecting => Task::none(),
            },
            Message::ModelsListed(Ok(models)) => self.update_models(models),
            Message::InputChanged(action) => {
                self.input.perform(action);

                Task::none()
            }
            Message::InputResized(size) => {
                self.input_height = size.height;

                Task::none()
            }
            Message::ContentResized(size) => {
                self.content_width = size.width;

                Task::none()
            }
            Message::SnapToBottom(snap_to_bottom) => {
                self.snap_to_bottom = snap_to_bottom;

                Task::none()
            }
            Message::Send => {
                let message = self.input.text();

                self.input = text_editor::Content::new();
                self.messages.push(Item::User(Markdown::new(message)));

                let work = if self.tasks.is_empty() {
                    self.work()
                } else {
                    self.abort();

                    // Wait for a couple seconds to server slots return to idle
                    // This is necessary for proper reuse of prompt caches
                    Task::future(tokio::time::sleep(time::seconds(2)))
                        .discard()
                        .chain(self.work())
                };

                Task::batch([work, operation::snap_to_end("scroll")])
            }
            Message::ReplyProgressed(event) => {
                let Some(Item::Assistant(reply)) = self
                    .messages
                    .iter_mut()
                    .rev()
                    .find(|message| !matches!(message, Item::Tool(_)))
                else {
                    return Task::none();
                };

                reply.timings = event.timings;

                let task = match event.delta {
                    reason::Delta::PromptProcessed(progress) => {
                        reply.prompt = progress;

                        Task::none()
                    }
                    reason::Delta::ReasoningChanged(delta) => {
                        reply.reasoning.push_str(&delta);

                        Task::none()
                    }
                    reason::Delta::ContentChanged(delta) => {
                        reply.content.push_str(&delta);

                        Task::none()
                    }
                    reason::Delta::ToolCallsChanged(deltas) => {
                        let mut calls = Vec::new();

                        for delta in deltas {
                            match delta {
                                reason::tool::Delta::CallAdded(call) => {
                                    let last_call = reply.tool_calls.last().cloned();
                                    reply.tool_calls.push(call);

                                    let Some(call) = last_call else {
                                        continue;
                                    };

                                    calls.push(call);
                                }
                                reason::tool::Delta::ArgumentsChanged(delta) => {
                                    let Some(tool_call) = reply.tool_calls.last_mut() else {
                                        continue;
                                    };

                                    tool_call.arguments.push_str(&delta);
                                }
                            }
                        }

                        Task::batch(calls.into_iter().map(|call| self.run(call)))
                    }
                };

                Task::batch([
                    task,
                    if self.snap_to_bottom {
                        operation::snap_to_end("scroll")
                    } else {
                        Task::none()
                    },
                ])
            }
            Message::ReplyReceived(Ok(_reply)) => {
                let _ = self.tasks.remove(&Work::Completion);

                let Some(Item::Assistant(reply)) = self
                    .messages
                    .iter()
                    .rev()
                    .find(|message| !matches!(message, Item::Tool(_)))
                else {
                    return Task::none();
                };

                if let Some(call) = reply.tool_calls.last().cloned() {
                    self.run(call)
                } else {
                    Task::none()
                }
            }
            Message::CompactionProgressed(event) => {
                let Some(Item::Compaction(compaction)) = self.messages.last_mut() else {
                    return Task::none();
                };

                let new_tokens = event
                    .timings
                    .zip(compaction.reply.timings)
                    .map(|(now, old)| now.total_tokens().saturating_sub(old.total_tokens()))
                    .unwrap_or(0);

                compaction.reply.timings = event.timings;

                match event.delta {
                    reason::Delta::PromptProcessed(progress) => {
                        compaction.reply.prompt = progress;
                    }
                    reason::Delta::ReasoningChanged(delta) => {
                        compaction.reply.reasoning.push_str(&delta);
                        compaction.reasoning_tokens += new_tokens;
                    }
                    reason::Delta::ContentChanged(delta) => {
                        compaction.reply.content.push_str(&delta);
                        compaction.tokens += new_tokens;
                    }
                    reason::Delta::ToolCallsChanged(_) => {}
                }

                Task::none()
            }
            Message::CompactionReceived(Ok(_reply)) => {
                let _ = self.tasks.remove(&Work::Compaction);

                let Some(Item::Compaction(compaction)) = self.messages.last_mut() else {
                    return Task::none();
                };

                compaction.reply.timings = Some(reason::Timings::default());
                compaction.is_finished = true;

                log::trace!(
                    "compaction received: content_len={} reasoning_len={}",
                    _reply.content.len(),
                    _reply.reasoning.len()
                );

                self.work()
            }
            Message::LinkClicked(uri) => {
                log::debug!("{uri:?}");

                Task::none()
            }
            Message::ToolProgressed(id, line) => {
                let Some(tool) = self.messages.iter_mut().rev().find_map(|message| {
                    if let Item::Tool(tool) = message
                        && tool.call.id == id
                    {
                        Some(tool)
                    } else {
                        None
                    }
                }) else {
                    return Task::none();
                };

                let Status::Running { logs } = &mut tool.status else {
                    return Task::none();
                };

                logs.push(line);

                if tool.snap_to_bottom {
                    operation::snap_to_end(tool.call.id.as_str().to_owned())
                } else {
                    Task::none()
                }
            }
            Message::ToolScrolled(id, snap_to_bottom) => {
                let Some(tool) = self.messages.iter_mut().rev().find_map(|message| {
                    if let Item::Tool(tool) = message
                        && tool.call.id == id
                    {
                        Some(tool)
                    } else {
                        None
                    }
                }) else {
                    return Task::none();
                };

                tool.snap_to_bottom = snap_to_bottom;

                Task::none()
            }
            Message::ToolFinished(id, result) => {
                let Some(tool) = self.messages.iter_mut().rev().find_map(|message| {
                    if let Item::Tool(tool) = message
                        && tool.call.id == id
                    {
                        Some(tool)
                    } else {
                        None
                    }
                }) else {
                    return Task::none();
                };

                let _ = self.tasks.remove(&Work::Tool(id));

                if !matches!(tool.status, Status::Running { .. }) {
                    return Task::none();
                }

                tool.status = match result {
                    Ok(output) => Status::Success { output },
                    Err(error) => Status::Error {
                        output: error.to_string(),
                    },
                };

                if self.tasks.is_empty() {
                    self.work()
                } else {
                    Task::none()
                }
            }
            Message::Abort => {
                self.abort();

                Task::none()
            }
            Message::Connected(Err(error))
            | Message::ModelsListed(Err(error))
            | Message::ReplyReceived(Err(error))
            | Message::CompactionReceived(Err(error)) => {
                self.connection = Connection::Disconnected;
                self.abort();

                log::error!("{error}");

                Task::none()
            }
        }
    }

    fn abort(&mut self) {
        self.tasks.clear();

        for message in &mut self.messages {
            if let Item::Tool(tool) = message
                && matches!(tool.status, Status::Running { .. })
            {
                tool.status = Status::Aborted;
            }
        }

        let index = self.messages.iter().rposition(|message| {
            matches!(
                message,
                Item::Compaction(Compaction {
                    is_finished: false,
                    ..
                })
            )
        });

        if let Some(index) = index {
            self.messages.remove(index);
        }
    }

    fn work(&mut self) -> Task<Message> {
        use iced::task::{Sipper, sipper};

        if !self.tasks.is_empty() {
            return Task::none();
        }

        if let Some(compact) = self.compact() {
            return compact;
        }

        let Connection::Connected(reason) = &self.connection else {
            return Task::none();
        };

        let Some(model) = &self.model else {
            return Task::none();
        };

        let reason = reason.clone();
        let model = model.clone();

        let messages: Vec<_> = self
            .opener()
            .chain(self.context().iter().filter_map(Item::to_message))
            .collect();

        for message in &messages {
            log::trace!("{message:?}");
        }

        let tools: Vec<_> = self.tools().collect();

        let (reply, handle) = Task::sip(
            sipper(async move |sender| reason.reply(&model, &messages, &tools).run(sender).await),
            Message::ReplyProgressed,
            Message::ReplyReceived,
        )
        .abortable();

        self.tasks.insert(Work::Completion, handle.abort_on_drop());
        self.messages.push(Item::Assistant(Reply::default()));

        reply
    }

    fn run(&mut self, call: reason::tool::Call) -> Task<Message> {
        let state = match self.tools.get(call.name.as_str()) {
            Some(tool) => tool.parse(&call.arguments),
            None => {
                Err(std::io::Error::other(format!("unknown tool: {name}", name = call.name)).into())
            }
        };

        let (run, status) = match &state {
            Ok(state) => {
                let run = state.run(&self.project);

                let (run, handle) = Task::sip(
                    run,
                    Message::ToolProgressed.with(call.id.clone()),
                    Message::ToolFinished.with(call.id.clone()),
                )
                .abortable();

                self.tasks
                    .insert(Work::Tool(call.id.clone()), handle.abort_on_drop());

                (run, Status::Running { logs: Vec::new() })
            }
            Err(_error) => (Task::none(), Status::Invalid),
        };

        self.messages.push(Item::Tool(ToolRun {
            call,
            state,
            status,
            snap_to_bottom: true,
        }));

        run
    }

    fn compact(&mut self) -> Option<Task<Message>> {
        use iced::task::{Sipper, sipper};

        /// Minimum amount of tokens compaction needs
        const COMPACTION_MIN_TOKENS: u64 = 4_000;
        /// Maximum amount of tokens compaction needs
        const COMPACTION_MAX_TOKENS: u64 = 10_000;
        /// Factor of context from last message to keep intact
        const CONTINUITY_CONTEXT: f32 = 0.2;

        const COMPACTION_PROMPT: &str = r#"The earlier part of this conversation is about to be removed to free up context.
After your next message, only your summary of it will remain, embedded in your system prompt for the rest of the session.

Write a concise state handoff so the work can continue seamlessly without the original messages. Cover:
- The user's goal, and any explicit constraints or preferences
- Key decisions and their rationale (including rejected alternatives)
- Current state: files created or modified and why, what is done, what is in progress
- Errors encountered and how they were resolved or to be avoided
- Open questions and the immediate next steps
Omit chit-chat, raw tool output, and failed experiments (keep only the lesson).
If your system prompt already contains a previous summary, merge it into this one; the result must be self-contained.
Reply with only the summary, under 500 words. You cannot use any tools."#;

        let Connection::Connected(reason) = &self.connection else {
            return None;
        };

        let model = self.model.as_ref()?;
        let context_size = self.context_size()?;

        let timings = self.timings()?;

        let total_tokens = timings.total_tokens();
        let context_left = context_size.saturating_sub(total_tokens);
        let budget = (context_size / 8).clamp(COMPACTION_MIN_TOKENS, COMPACTION_MAX_TOKENS);

        if context_left > budget {
            return None;
        }

        let tokens_to_keep =
            ((total_tokens as f32 * CONTINUITY_CONTEXT).round() as u64).max(budget / 2);
        let context = self.context();

        // Find target reply
        let target = context
            .iter()
            .rev()
            .position(|item| {
                let Item::Assistant(Reply {
                    timings: Some(timings),
                    ..
                }) = item
                else {
                    return false;
                };

                log::trace!(
                    "compact candidate: total={} reply={}",
                    total_tokens,
                    timings.total_tokens()
                );

                total_tokens.saturating_sub(timings.total_tokens()) >= tokens_to_keep
            })
            .unwrap_or(context.len());

        // Compact tools as well
        let tools = context[context.len() - target..]
            .iter()
            .take_while(|item| matches!(item, Item::Tool(_)))
            .count();

        let end = context.len() - target + tools;

        log::debug!(
            "compact: total_tokens={} context_left={} context.len={} target={} end={}",
            total_tokens,
            context_left,
            context.len(),
            target,
            end
        );

        let prefix: Vec<_> = self
            .opener()
            .chain(context[..end].iter().filter_map(Item::to_message))
            .chain([reason::Message::User(COMPACTION_PROMPT.to_owned())])
            .collect();

        let reason = reason.clone();
        let model = model.clone();
        let tools: Vec<_> = self.tools().collect();

        self.messages.push(Item::Compaction(Compaction {
            reply: Reply::default(),
            tokens: 0,
            reasoning_tokens: 0,
            to: (self.messages.len() - context.len()) + end,
            is_finished: false,
        }));

        let (reply, handle) = Task::sip(
            sipper(async move |sender| reason.reply(&model, &prefix, &tools).run(sender).await),
            Message::CompactionProgressed,
            Message::CompactionReceived,
        )
        .abortable();

        self.tasks.insert(Work::Compaction, handle.abort_on_drop());

        Some(reply)
    }

    fn view(&self) -> Element<'_, Message> {
        let conversation: Element<'_, Message> = if self.messages.is_empty() {
            sensor(center(text("Ready when you are.").size(TITLE).center()))
                .on_resize(Message::ContentResized)
                .into()
        } else {
            scrollable(
                sensor(center_x(
                    column(self.messages.iter().map(Item::view))
                        .spacing(20)
                        .width(Fit.max(MAX_WIDTH))
                        .padding(padding::bottom(self.input_height + 20.0)),
                ))
                .on_resize(|size| {
                    (size.width != self.content_width).then_some(Message::ContentResized(size))
                }),
            )
            .id("scroll")
            .width(Fill)
            .height(Fill)
            .on_scroll(|viewport| {
                let snap_to_bottom = snap_to_bottom(viewport);

                (self.snap_to_bottom != snap_to_bottom)
                    .then_some(Message::SnapToBottom(snap_to_bottom))
            })
            .spacing(20)
            .into()
        };

        let input = text_editor(&self.input)
            .id("input")
            .height(Fit.max(600))
            .padding(10)
            .placeholder("Type your query here...")
            .on_action(Message::InputChanged)
            .key_binding(|key_press| {
                if key_press.key == keyboard::Key::Named(keyboard::key::Named::Escape) {
                    return Some(text_editor::Binding::Custom(Message::Abort));
                }

                if !key_press.is_focused {
                    return None;
                }

                if key_press.key == keyboard::Key::Named(keyboard::key::Named::Enter)
                    && !key_press.modifiers.shift()
                {
                    return Some(text_editor::Binding::Custom(Message::Send));
                }

                text_editor::Binding::from_key_press(key_press)
            });

        let status = {
            let project = tildify(&self.project, self.home.as_deref());

            let server = {
                let timings = self.timings();

                let info = timings.map(|timings| {
                    row![
                        (timings.prompt.token > time::Duration::ZERO).then(|| {
                            text!(
                                "{tokens_per_second:0.2}↑",
                                tokens_per_second = 1.0 / timings.prompt.token.as_secs_f64(),
                            )
                            .style(text::secondary)
                            .size(SMALL)
                        }),
                        (timings.predicted.token > time::Duration::ZERO).then(|| {
                            text!(
                                "↓{tokens_per_second:0.2}",
                                tokens_per_second = 1.0 / timings.predicted.token.as_secs_f64(),
                            )
                            .style(text::success)
                            .size(SMALL)
                        })
                    ]
                    .spacing(10)
                });

                let models = if let Some(model) = self.model.as_ref() {
                    text(model.as_str())
                } else {
                    text("No models found!")
                }
                .size(SMALL)
                .width(Fit.max(200))
                .wrapping(text::Wrapping::None)
                .ellipsis(text::Ellipsis::End)
                .style(|theme: &Theme| {
                    let palette = theme.seed();

                    text::Style {
                        color: match &self.connection {
                            Connection::Disconnected => Some(palette.danger),
                            Connection::Connecting => Some(palette.warning),
                            Connection::Connected(_) => None,
                        },
                    }
                });

                let context = context_led(self.context_size(), timings);

                row![info, models, context].spacing(10).align_y(Center)
            };

            row![
                text(project.display().to_string()).size(SMALL),
                space::horizontal(),
                server,
            ]
            .align_y(Center)
            .spacing(10)
        };

        container(stack![
            conversation,
            bottom(
                center_x(
                    sensor(column![input, status].spacing(10).width(Fit.max(MAX_WIDTH)))
                        .on_resize(Message::InputResized)
                )
                .style(|theme| container::Style {
                    background: Some(theme.seed().background.into()),
                    ..container::transparent(theme)
                })
                .width(self.content_width)
            )
        ])
        .padding(10)
        .into()
    }

    fn subscription(&self) -> Subscription<Message> {
        time::every(time::seconds(10)).map(|_| Message::Reconnect)
    }

    fn context_size(&self) -> Option<u64> {
        self.model
            .as_ref()
            .and_then(|model| self.models.get(model))
            .and_then(|model| model.context_size)
    }

    fn context(&self) -> &[Item] {
        let start = self
            .last_compaction()
            .map(|compaction| compaction.to)
            .unwrap_or_default();

        &self.messages[start..]
    }

    fn last_compaction(&self) -> Option<&Compaction> {
        self.messages.iter().rev().find_map(|item| {
            let Item::Compaction(
                compaction @ Compaction {
                    is_finished: true, ..
                },
            ) = item
            else {
                return None;
            };

            Some(compaction)
        })
    }

    fn system_prompt(&self) -> String {
        const PROMPT: &str = "You are an expert coding assistant. \
            The user wants your help to develop a project in the current directory.";

        if let Some(compaction) = self.last_compaction() {
            format!("{PROMPT}\n\n{}", compaction.reply.content.raw())
        } else {
            PROMPT.to_owned()
        }
    }

    /// The system prompt, plus the most recent user message before the
    /// compaction cutoff — the opener of the turn the boundary falls in —
    /// so the model retains the verbatim request that the summary only
    /// paraphrases.
    fn opener(&self) -> impl Iterator<Item = reason::Message> {
        let start = self.messages.len() - self.context().len();

        std::iter::once(reason::Message::System(self.system_prompt())).chain(
            if let Some(Item::Assistant(_) | Item::Compaction(_)) = self.messages.get(start)
                && let Some(Item::User(markdown)) = self.messages[..start]
                    .iter()
                    .rev()
                    .find(|item| matches!(item, Item::User(_)))
            {
                Some(reason::Message::User(markdown.raw().to_owned()))
            } else {
                None
            },
        )
    }

    fn tools(&self) -> impl Iterator<Item = reason::Tool> {
        self.tools.values().map(Tool::to_metadata)
    }

    fn timings(&self) -> Option<reason::Timings> {
        self.context().iter().rev().find_map(|item| {
            if let Item::Assistant(reply) | Item::Compaction(Compaction { reply, .. }) = item {
                reply.timings
            } else {
                None
            }
        })
    }
}

const TITLE: u32 = 20;
const NORMAL: u32 = 16;
const SMALL: u32 = 14;
const MAX_WIDTH: u32 = 770;
const TOOL_LOG_LINE_HEIGHT: f32 = 18.0;
const MAX_TOOL_LOG_HEIGHT: f32 = TOOL_LOG_LINE_HEIGHT * 10.0 + 5.0 * 9.0; // 10 lines: 10 × 18px + 9 × 5px spacing
const SNAP_TO_BOTTOM: f32 = 20.0;

fn snap_to_bottom(viewport: scrollable::Viewport) -> bool {
    let offset = viewport.absolute_offset();
    let bounds = viewport.bounds();
    let content_bounds = viewport.content_bounds();

    let distance_to_bottom = (content_bounds.height - bounds.height - offset.y).max(0.0);

    distance_to_bottom <= SNAP_TO_BOTTOM
}

fn tildify(path: &Path, home: Option<&Path>) -> PathBuf {
    let Some(home) = home else {
        return path.to_path_buf();
    };

    match path.strip_prefix(home) {
        Ok(rest) if rest.as_os_str().is_empty() => PathBuf::from("~"),
        Ok(rest) => Path::new("~").join(rest),
        Err(_) => path.to_path_buf(),
    }
}

fn context_led<'a>(
    context_size: Option<u64>,
    timings: Option<reason::Timings>,
) -> Element<'a, Message> {
    use iced::mouse;
    use iced::widget::{canvas, tooltip};
    use iced::{Radians, Rectangle, Renderer};

    use std::cell::RefCell;
    use std::f32::consts::{FRAC_PI_2, PI};

    const SIZE: f32 = 14.0;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    struct Led {
        context_size: Option<u64>,
        timings: Option<reason::Timings>,
    }

    #[derive(Default)]
    struct State {
        last: RefCell<Led>,
        cache: canvas::Cache,
    }

    impl canvas::Program<Message> for Led {
        type State = State;

        fn draw(
            &self,
            state: &Self::State,
            renderer: &Renderer,
            theme: &Theme,
            bounds: Rectangle,
            _cursor: mouse::Cursor,
        ) -> Vec<canvas::Geometry> {
            const STROKE_WIDTH: f32 = 2.0;

            if *state.last.borrow() != *self {
                *state.last.borrow_mut() = *self;
                state.cache.clear();
            }

            let geometry = state.cache.draw(renderer, bounds.size(), |frame| {
                let palette = theme.palette();
                let radius = (frame.width() - STROKE_WIDTH) / 2.0;
                let circle = canvas::Path::circle(frame.center(), radius);

                frame.stroke(
                    &circle,
                    canvas::Stroke {
                        style: canvas::Style::Solid(palette.background.strong.color),
                        width: STROKE_WIDTH,
                        ..canvas::Stroke::default()
                    },
                );

                if let Some(timings) = self.timings
                    && let Some(context_size) = self.context_size
                {
                    let usage = timings.total_tokens() as f32 / context_size as f32;

                    let arc = {
                        let mut builder = canvas::path::Builder::new();

                        let start = -FRAC_PI_2;

                        builder.arc(canvas::path::Arc {
                            center: frame.center(),
                            radius,
                            start_angle: Radians(start),
                            end_angle: Radians(start + 2.0 * PI * usage),
                        });

                        builder.build()
                    };

                    frame.stroke(
                        &arc,
                        canvas::Stroke {
                            style: canvas::Style::Solid(match usage {
                                0.0..0.8 => palette.primary.base.color,
                                0.8..0.9 => palette.warning.base.color,
                                _ => palette.danger.base.color,
                            }),
                            width: STROKE_WIDTH,
                            line_cap: canvas::LineCap::Square,
                            ..canvas::Stroke::default()
                        },
                    );
                }
            });

            vec![geometry]
        }
    }

    let led = canvas(Led {
        timings,
        context_size,
    })
    .width(SIZE)
    .height(SIZE);

    match (context_size, timings) {
        (Some(context_size), Some(timings)) => {
            let tokens = timings.total_tokens();
            let percent = tokens as f32 / context_size as f32 * 100.0;

            tooltip(
                led,
                text!(
                    "{} / {} ({percent:.1}%)",
                    thousands(tokens),
                    thousands(context_size)
                )
                .size(SMALL),
                tooltip::Position::Top,
            )
            .style(container::rounded_box)
            .into()
        }
        _ => led.into(),
    }
}

fn thousands(value: u64) -> String {
    let digits = value.to_string();
    let mut formatted = String::with_capacity(digits.len() + digits.len() / 3);

    for (index, digit) in digits.char_indices() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            formatted.push(',');
        }

        formatted.push(digit);
    }

    formatted
}

fn duration(duration: time::Duration) -> String {
    let seconds = duration.as_secs_f64();

    if seconds < 1.0 {
        format!("{}ms", duration.as_millis())
    } else if seconds < 60.0 {
        format!("{seconds:.1}s")
    } else {
        let total = seconds.round() as u64;
        let (hours, minutes) = (total / 3600, (total % 3600) / 60);
        let seconds = total % 60;

        if hours > 0 {
            format!("{hours}h {minutes}m {seconds}s")
        } else {
            format!("{minutes}m {seconds}s")
        }
    }
}

fn prompt_progress<'a>(progress: reason::Progress) -> Option<Element<'a, Message>> {
    if progress.total == progress.processed {
        return None;
    }

    Some(
        center_x(
            progress_bar(0.0..=1.0, progress.processed as f32 / progress.total as f32)
                .girth(10)
                .length(100)
                .style(progress_bar::secondary),
        )
        .into(),
    )
}
