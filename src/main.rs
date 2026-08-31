mod tool;

use crate::tool::Tool;

use iced::keyboard;
use iced::padding;
use iced::task;
use iced::time;
use iced::widget::operation;
use iced::widget::{
    bottom, center, center_x, column, container, markdown, right, row, scrollable, sensor, space,
    stack, text, text_editor,
};
use iced::{Center, Element, Fill, Fit, Font, Size, Subscription, Task, Theme};

use function::Binary;
use reason::Reason;

use std::collections::HashMap;
use std::env;
use std::path::{Path, PathBuf};

fn main() -> Result<(), iced::Error> {
    tracing_subscriber::fmt::init();

    iced::application(Pick::new, Pick::update, Pick::view)
        .subscription(Pick::subscription)
        .theme(Theme::CatppuccinMocha)
        .default_font(Font::MONOSPACE)
        .run()
}

struct Pick {
    connection: Connection,
    models: Vec<reason::Model>,
    model: Option<reason::Model>,
    messages: Vec<Item>,
    input: text_editor::Content,
    input_height: f32,
    content_width: f32,
    snap_to_bottom: bool,
    completion: Option<task::Handle>,
    tools: HashMap<&'static str, Tool>,
    project: PathBuf,
    home: Option<PathBuf>,
    server: String,
}

enum Item {
    User(Markdown),
    Assistant(Reply),
    Tool(ToolRun),
}

impl Item {
    fn to_message(&self) -> reason::Message {
        match self {
            Item::User(markdown) => reason::Message::User(markdown.raw.clone()),
            Item::Assistant(reply) => reason::Message::Assistant(reason::Reply {
                reasoning: reply.reasoning.raw.clone(),
                content: reply.content.raw.clone(),
                tool_calls: reply.tool_calls.clone(),
            }),
            Item::Tool(tool) => reason::Message::Tool(reason::tool::Response {
                id: tool.call.id.clone(),
                content: tool.status.content().unwrap_or_default().to_owned(),
            }),
        }
    }

    fn view(&self) -> Element<'_, Message> {
        match self {
            Item::Assistant(reply) => {
                let reasoning = if !reply.reasoning.raw.is_empty() {
                    Some(
                        container(reply.reasoning.view()).style(|theme| container::Style {
                            text_color: Some(theme.palette().secondary.strong.color),
                            ..container::transparent(theme)
                        }),
                    )
                } else {
                    None
                };

                column![
                    reasoning,
                    (!reply.content.raw.is_empty()).then(|| reply.content.view())
                ]
                .spacing(10)
                .into()
            }
            Item::User(message) => right(
                container(message.view())
                    .padding(10)
                    .style(container::rounded_box),
            )
            .into(),
            Item::Tool(tool) => container(
                column![
                    text!(
                        "{name}({arguments})",
                        name = tool.call.name,
                        arguments = tool.call.arguments
                    )
                    .size(SMALL),
                    container(tool.status.content().map(|content| {
                        if content.len() > 50 {
                            text(format!(
                                "{}...",
                                &content[0..content.floor_char_boundary(50)]
                            ))
                        } else {
                            text(content)
                        }
                        .size(SMALL)
                    }))
                    .width(Fill)
                    .padding(10)
                    .style(container::dark)
                ]
                .spacing(10),
            )
            .width(Fill)
            .padding(10)
            .style(|theme: &Theme| {
                let palette = theme.seed();

                let color = match tool.status {
                    Status::Running => palette.warning,
                    Status::Success(_) => palette.success.scale_alpha(0.5),
                    Status::Error(_) => palette.danger,
                };

                let mut style = container::bordered_box(theme);
                style.border = style.border.color(color);
                style
            })
            .into(),
        }
    }
}

#[derive(Debug, Default)]
struct Markdown {
    raw: String,
    content: markdown::Content,
}

impl Markdown {
    fn new(raw: String) -> Self {
        Self {
            content: markdown::Content::parse(&raw),
            raw,
        }
    }

    fn push_str(&mut self, delta: &str) {
        self.raw.push_str(delta);
        self.content.push_str(delta);
    }

    fn view(&self) -> Element<'_, Message> {
        markdown(
            self.content.items(),
            markdown::Settings::with_text_size(
                16,
                markdown::Style {
                    font: Font::MONOSPACE,
                    ..markdown::Style::from_palette(Theme::CatppuccinMocha.seed())
                },
            ),
        )
        .map(Message::LinkClicked)
    }
}

#[derive(Debug, Default)]
struct Reply {
    reasoning: Markdown,
    content: Markdown,
    tool_calls: Vec<reason::tool::Call>,
}

#[derive(Debug)]
struct ToolRun {
    call: reason::tool::Call,
    status: Status,
}

#[derive(Debug)]
enum Status {
    Running,
    Success(String),
    Error(String),
}

impl Status {
    fn content(&self) -> Option<&str> {
        match self {
            Status::Running => None,
            Status::Success(output) | Status::Error(output) => Some(output.as_str()),
        }
    }
}

#[derive(Debug, Clone)]
enum Connection {
    Disconnected,
    Connecting,
    Connected(Reason),
}

#[derive(Debug, Clone)]
enum Message {
    Connected(Result<Reason, reason::Error>),
    ModelsListed(Result<Vec<reason::Model>, reason::Error>),
    Reconnect,
    InputChanged(text_editor::Action),
    InputResized(Size),
    ContentResized(Size),
    ContentScrolled(scrollable::Viewport),
    Send,
    ReplyProgressed(reason::Event),
    ReplyReceived(Result<reason::Reply, reason::Error>),
    LinkClicked(markdown::Uri),
    ToolFinished(usize, Result<String, reason::Error>),
}

impl Pick {
    fn new() -> (Self, Task<Message>) {
        let mut pick = Self {
            connection: Connection::Disconnected,
            models: Vec::new(),
            model: None,
            messages: Vec::new(),
            input: text_editor::Content::new(),
            input_height: 0.0,
            content_width: 0.0,
            snap_to_bottom: true,
            completion: None,
            project: env::current_dir().unwrap_or_default(),
            home: env::home_dir(),
            server: "http://127.0.0.1:9931".to_owned(),
            tools: Tool::builtins(),
        };

        let connect = pick.connect();

        (pick, Task::batch([connect, operation::focus("input")]))
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

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Connected(Ok(reason)) => {
                self.connection = Connection::Connected(reason);
                self.list_models()
            }
            Message::Reconnect => match &self.connection {
                Connection::Disconnected => self.connect(),
                Connection::Connected(_) => self.list_models(),
                Connection::Connecting => Task::none(),
            },
            Message::ModelsListed(Ok(models)) => {
                self.models = models;

                if self
                    .model
                    .as_ref()
                    .is_none_or(|model| !self.models.contains(model))
                {
                    self.model = self.models.first().cloned();
                }

                Task::none()
            }
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
            Message::ContentScrolled(viewport) => {
                self.snap_to_bottom = viewport.relative_offset().y > 0.98;

                Task::none()
            }
            Message::Send => {
                let message = self.input.text();

                self.input = text_editor::Content::new();
                self.messages.push(Item::User(Markdown::new(message)));

                self.completion = None;
                self.work()
            }
            Message::ReplyProgressed(event) => {
                let Some(Item::Assistant(reply)) = self.messages.last_mut() else {
                    return Task::none();
                };

                match event {
                    reason::Event::ReasoningChanged { delta, .. } => {
                        reply.reasoning.push_str(&delta);
                    }
                    reason::Event::ContentChanged { delta, .. } => {
                        reply.content.push_str(&delta);
                    }
                    reason::Event::ToolCallAdded(call) => {
                        reply.tool_calls.push(call);
                    }
                    reason::Event::ArgumentsChanged { delta } => {
                        let Some(tool_call) = reply.tool_calls.last_mut() else {
                            return Task::none();
                        };

                        tool_call.arguments.push_str(&delta);
                    }
                }

                if self.snap_to_bottom {
                    operation::snap_to_end("scroll")
                } else {
                    Task::none()
                }
            }
            Message::ReplyReceived(Ok(_reply)) => {
                self.completion = None;

                let Some(Item::Assistant(reply)) = self.messages.last_mut() else {
                    return Task::none();
                };

                let tool_calls = reply.tool_calls.clone();
                let start = self.messages.len();

                let (run, handle) = Task::batch(
                    tool_calls
                        .iter()
                        .map(|call| match self.tools.get(call.name.as_str()) {
                            Some(tool) => tool.run(&self.project, &call.arguments),
                            None => {
                                let name = call.name.clone();

                                Box::pin(async move {
                                    Err(std::io::Error::other(format!("unknown tool: {name}")))?
                                })
                            }
                        })
                        .map(Task::future)
                        .enumerate()
                        .map(|(i, task)| task.map(Message::ToolFinished.with(start + i))),
                )
                .abortable();

                for call in tool_calls {
                    self.messages.push(Item::Tool(ToolRun {
                        call,
                        status: Status::Running,
                    }));
                }

                self.completion = Some(handle.abort_on_drop());

                run
            }
            Message::LinkClicked(uri) => {
                dbg!(uri);

                Task::none()
            }
            Message::ToolFinished(i, result) => {
                let Some(Item::Tool(tool)) = self.messages.get_mut(i) else {
                    return Task::none();
                };

                tool.status = match result {
                    Ok(output) => Status::Success(output),
                    Err(error) => Status::Error(error.to_string()),
                };

                let all_finished = self
                    .messages
                    .iter()
                    .rev()
                    .take_while(|message| matches!(message, Item::Tool(_)))
                    .all(|message| {
                        let Item::Tool(tool) = message else {
                            return true;
                        };

                        !matches!(tool.status, Status::Running)
                    });

                if all_finished {
                    self.work()
                } else {
                    Task::none()
                }
            }
            Message::Connected(Err(error))
            | Message::ModelsListed(Err(error))
            | Message::ReplyReceived(Err(error)) => {
                self.connection = Connection::Disconnected;
                self.completion = None;

                log::error!("{error}");

                Task::none()
            }
        }
    }

    fn work(&mut self) -> Task<Message> {
        use iced::task::{Sipper, sipper};

        if self.completion.is_some() {
            return Task::none();
        }

        let Connection::Connected(reason) = &self.connection else {
            return Task::none();
        };

        let Some(model) = &self.model else {
            return Task::none();
        };

        let reason = reason.clone();
        let model = model.clone();
        let messages: Vec<_> = [reason::Message::System(
            "You are an expert coding assistant. The user wants your help to develop a project in the current directory.".to_owned(),
        )]
        .into_iter()
        .chain(self.messages.iter().map(Item::to_message))
        .collect();

        let tools: Vec<_> = self.tools.values().map(Tool::to_metadata).collect();

        let (reply, handle) = Task::sip(
            sipper(async move |sender| reason.reply(&model, &messages, &tools).run(sender).await),
            Message::ReplyProgressed,
            Message::ReplyReceived,
        )
        .abortable();

        self.completion = Some(handle.abort_on_drop());
        self.messages.push(Item::Assistant(Reply::default()));

        reply
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
                        .padding(padding::bottom(self.input_height + 10.0)),
                ))
                .on_resize(Message::ContentResized),
            )
            .id("scroll")
            .width(Fill)
            .height(Fill)
            .on_scroll(Message::ContentScrolled)
            .spacing(10)
            .into()
        };

        let input = text_editor(&self.input)
            .id("input")
            .height(Fit.max(600))
            .padding(10)
            .placeholder("Type your query here...")
            .on_action(Message::InputChanged)
            .key_binding(|key_press| {
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
            let server = {
                let models = if let Some(model) = self.model.as_ref() {
                    text(model.as_str())
                } else {
                    text("No models found!").style(text::warning)
                }
                .size(SMALL)
                .width(Fit.max(200))
                .wrapping(text::Wrapping::None)
                .ellipsis(text::Ellipsis::End);

                let context = match &self.connection {
                    Connection::Disconnected => {
                        text("Disconnected").size(SMALL).style(text::danger)
                    }
                    Connection::Connecting => {
                        text("Connecting...").size(SMALL).style(text::warning)
                    }
                    Connection::Connected(_) => text("Connected").size(SMALL).style(text::success),
                };

                row![models, context].spacing(10).align_y(Center)
            };

            let project = tildify(&self.project, self.home.as_deref());

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
}

const TITLE: u32 = 20;
const SMALL: u32 = 14;
const MAX_WIDTH: u32 = 770;

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
