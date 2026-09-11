use crate::font;
use crate::locale;
use crate::markdown::{self, Markdown};
use crate::tool;
use crate::widget;

use iced::border;
use iced::padding;
use iced::time;
use iced::widget::{
    center_x, column, container, progress_bar, right, row, scrollable, stack, text,
};
use iced::{Center, Element, Fill, Fit, Font, Pixels, Task, Theme, never};

pub enum Item {
    User(Markdown),
    Assistant(Reply),
    Tool(ToolRun),
    Compaction(Compaction),
}

#[derive(Debug, Clone)]
pub enum Message {
    ToolLogSnapped(bool),
    LinkClicked(markdown::Uri),
}

impl Item {
    pub fn to_message(&self) -> Option<reason::Message> {
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

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::ToolLogSnapped(snap_to_bottom) => {
                let Item::Tool(tool) = self else {
                    return Task::none();
                };

                tool.snap_to_bottom = snap_to_bottom;

                Task::none()
            }
            Message::LinkClicked(uri) => {
                // TODO
                log::debug!("{uri}");
                Task::none()
            }
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        match self {
            Item::Assistant(reply) => {
                let reasoning = if !reply.reasoning.is_empty() {
                    const MAX_HEIGHT: f32 = font::SMALL * 1.5 * 15.0; // 15 lines

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
                        .size(font::SMALL)
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
                                    markdown::view(
                                        reply.reasoning.items(),
                                        Font::MONOSPACE,
                                        font::SMALL
                                    )
                                    .map(Message::LinkClicked)
                                )
                                .padding(padding::top(font::SMALL * 1.375 + 10.0)),
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
                        font::NORMAL
                    )
                    .map(Message::LinkClicked)),
                ]
                .spacing(10)
                .into()
            }
            Item::User(message) => right(
                container(
                    markdown::view(message.items(), Font::DEFAULT, font::NORMAL)
                        .map(Message::LinkClicked),
                )
                .padding(10)
                .style(container::rounded_box),
            )
            .into(),
            Item::Tool(tool) => {
                const TOOL_LOG_LINE_HEIGHT: f32 = 18.0;
                const MAX_TOOL_LOG_HEIGHT: f32 = TOOL_LOG_LINE_HEIGHT * 10.0 + 5.0 * 9.0; // 10 lines: 10 × 18px + 9 × 5px spacing

                let header = {
                    let label = container(text(&tool.call.name).size(font::SMALL))
                        .padding([2, 5])
                        .style(container::dark);

                    let title = tool
                        .state
                        .as_ref()
                        .ok()
                        .and_then(|state| Some(text(state.title()?).size(font::SMALL)));

                    row![label, title].spacing(10).align_y(Center)
                };

                let arguments = match &tool.state {
                    Ok(state) => state.view().map(|state| state.map(never)),
                    Err(error) => Some(
                        text!("{error}")
                            .size(font::SMALL)
                            .style(text::danger)
                            .into(),
                    ),
                };

                let output: Option<Element<'_, _>> = match &tool.status {
                    Status::Running { logs } => Some(
                        scrollable(
                            column(logs.iter().map(|line| {
                                text(&line[..line.floor_char_boundary(200)])
                                    .wrapping(text::Wrapping::None)
                                    .ellipsis(text::Ellipsis::End)
                                    .size(font::SMALL)
                                    .line_height(Pixels(TOOL_LOG_LINE_HEIGHT))
                                    .into()
                            }))
                            .spacing(5),
                        )
                        .id(tool.call.id.as_str().to_owned())
                        .width(Fill)
                        .height(Fit.max(MAX_TOOL_LOG_HEIGHT))
                        .on_scroll(|viewport| {
                            let snap_to_bottom = widget::snaps(viewport);

                            (tool.snap_to_bottom != snap_to_bottom)
                                .then_some(Message::ToolLogSnapped(snap_to_bottom))
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
                                .size(font::SMALL)
                                .into()
                        }

                        let total = output.lines();

                        Some(
                            if total > HEAD + TAIL {
                                let elided = total - HEAD - TAIL;

                                let marker = match elided {
                                    1 => "... 1 line elided".to_owned(),
                                    elided => {
                                        format!(
                                            "... {} lines elided",
                                            locale::thousands(elided as u64)
                                        )
                                    }
                                };

                                column(
                                    output
                                        .head(HEAD)
                                        .map(line)
                                        .chain(std::iter::once(
                                            text(marker)
                                                .size(font::SMALL)
                                                .style(text::secondary)
                                                .into(),
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
                        .size(font::SMALL)
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
pub struct Reply {
    pub prompt: reason::Progress,
    pub reasoning: Markdown,
    pub content: Markdown,
    pub tool_calls: Vec<reason::tool::Call>,
    pub timings: Option<reason::Timings>,
}

pub struct ToolRun {
    pub call: reason::tool::Call,
    pub state: Result<Box<dyn tool::Call>, reason::Error>,
    pub status: Status,
    pub snap_to_bottom: bool,
}

#[derive(Debug)]
pub enum Status {
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

pub struct Compaction {
    pub reply: Reply,
    pub tokens: u64,
    pub reasoning_tokens: u64,
    pub to: usize,
    pub is_finished: bool,
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
