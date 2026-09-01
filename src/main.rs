mod tool;

use crate::tool::Tool;

use iced::keyboard;
use iced::padding;
use iced::task;
use iced::time;
use iced::widget::operation;
use iced::widget::{
    bottom, center, center_x, column, container, markdown, progress_bar, right, row, scrollable,
    sensor, space, stack, text, text_editor,
};
use iced::{Center, Element, Fill, Fit, Font, Size, Subscription, Task, Theme};

use function::Binary;
use reason::Reason;
use reason::model;

use std::collections::{BTreeMap, HashMap};
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
    Tool(reason::tool::Id),
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

                let prompt_progress = if reply.prompt.total != reply.prompt.processed {
                    let progress = center_x(
                        progress_bar(
                            0.0..=1.0,
                            (reply.prompt.processed - reply.prompt.cached) as f32
                                / (reply.prompt.total - reply.prompt.cached) as f32,
                        )
                        .girth(10)
                        .length(100)
                        .style(progress_bar::secondary),
                    );

                    Some(progress)
                } else {
                    None
                };

                column![
                    prompt_progress,
                    reasoning,
                    (!reply.content.raw.is_empty()).then(|| reply.content.view()),
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
    prompt: reason::Progress,
    reasoning: Markdown,
    content: Markdown,
    tool_calls: Vec<reason::tool::Call>,
    timings: Option<reason::Timings>,
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
    ToolFinished(reason::tool::Id, Result<String, reason::Error>),
}

impl Pick {
    fn new() -> (Self, Task<Message>) {
        let mut pick = Self {
            connection: Connection::Disconnected,
            models: BTreeMap::new(),
            model: None,
            messages: Vec::new(),
            input: text_editor::Content::new(),
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
                let offset = viewport.absolute_offset();
                let bounds = viewport.bounds();
                let content_bounds = viewport.content_bounds();

                let distance_to_bottom =
                    (content_bounds.height - bounds.height - offset.y).max(0.0);

                self.snap_to_bottom = distance_to_bottom <= SNAP_TO_BOTTOM;

                Task::none()
            }
            Message::Send => {
                let message = self.input.text();

                self.input = text_editor::Content::new();
                self.messages.push(Item::User(Markdown::new(message)));

                let work = if self.tasks.is_empty() {
                    self.work()
                } else {
                    self.tasks.clear();

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
            Message::LinkClicked(uri) => {
                dbg!(uri);

                Task::none()
            }
            Message::ToolFinished(id, result) => {
                let Some(tool) = self.messages.iter_mut().find_map(|message| {
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

                tool.status = match result {
                    Ok(output) => Status::Success(output),
                    Err(error) => Status::Error(error.to_string()),
                };

                let _ = self.tasks.remove(&Work::Tool(id));

                if self.tasks.is_empty() {
                    self.work()
                } else {
                    Task::none()
                }
            }
            Message::Connected(Err(error))
            | Message::ModelsListed(Err(error))
            | Message::ReplyReceived(Err(error)) => {
                self.connection = Connection::Disconnected;
                self.tasks.clear();

                log::error!("{error}");

                Task::none()
            }
        }
    }

    fn work(&mut self) -> Task<Message> {
        use iced::task::{Sipper, sipper};

        if !self.tasks.is_empty() {
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

        self.tasks.insert(Work::Completion, handle.abort_on_drop());
        self.messages.push(Item::Assistant(Reply::default()));

        reply
    }

    fn run(&mut self, call: reason::tool::Call) -> Task<Message> {
        let (run, handle) = Task::perform(
            match self.tools.get(call.name.as_str()) {
                Some(tool) => tool.run(&self.project, &call.arguments),
                None => {
                    let name = call.name.clone();

                    Box::pin(
                        async move { Err(std::io::Error::other(format!("unknown tool: {name}")))? },
                    )
                }
            },
            Message::ToolFinished.with(call.id.clone()),
        )
        .abortable();

        self.tasks.insert(Work::Tool(call.id.clone()), handle);
        self.messages.push(Item::Tool(ToolRun {
            call,
            status: Status::Running,
        }));

        run
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
            let project = tildify(&self.project, self.home.as_deref());

            let server = {
                let timings = self.messages.iter().rev().find_map(|item| {
                    if let Item::Assistant(reply) = item {
                        reply.timings
                    } else {
                        None
                    }
                });

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

                let context_size = self
                    .model
                    .as_ref()
                    .and_then(|model| self.models.get(model))
                    .and_then(|model| model.context_size);

                let context = context_led(context_size, timings);

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
}

const TITLE: u32 = 20;
const SMALL: u32 = 14;
const MAX_WIDTH: u32 = 770;
const SNAP_TO_BOTTOM: f32 = 20.0;

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
                    let tokens_used =
                        timings.cached + timings.prompt.amount + timings.predicted.amount;

                    let usage = tokens_used as f32 / context_size as f32;

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
            let tokens = timings.cached + timings.prompt.amount + timings.predicted.amount;
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
