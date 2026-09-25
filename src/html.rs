//! Exports a session as a standalone HTML document, styled by
//! the app's theme.

use crate::core::{Session, git, session};
use crate::{diff, font, highlight, item, locale, tool};
use iced::highlighter;
use iced::theme::palette;
use iced::{Color, Theme};

/// The static styles of the document, colored by the `:root`
/// variables generated from the theme.
const CSS: &str = r#"
* {
    box-sizing: border-box;
}

body {
    margin: 0;
    background: var(--background);
    color: var(--text);
    font: 15px/1.6 system-ui, -apple-system, "Segoe UI", sans-serif;
}

.session {
    max-width: 770px;
    margin: 0 auto;
    padding: 40px 16px;
}

.session-header {
    color: var(--subtext);
    font-size: 13px;
    margin-bottom: 24px;
}

.item {
    margin-bottom: 15px;
}

.item.user {
    display: flex;
    justify-content: flex-end;
}

.bubble {
    background: var(--bubble);
    color: var(--bubble-text);
    border-radius: 2px;
    padding: 10px;
    max-width: 100%;
}

.reasoning {
    background: var(--card);
    color: var(--reasoning);
    border: 1px solid var(--border);
    border-radius: 5px;
    padding: 10px;
    font-family: ui-monospace, monospace;
    font-size: 13px;
}

.reasoning summary {
    font-weight: bold;
    cursor: pointer;
}

.reasoning div {
    margin-top: 10px;
}

/* A step groups one assistant turn: its last message is shown, and
   the reasoning and any earlier actions sit behind the header. */
.step {
    margin-bottom: 10px;
}

.step-header {
    display: flex;
    gap: 8px;
    align-items: baseline;
    padding: 3px 0;
    font-family: ui-monospace, monospace;
    font-size: 12px;
}

summary.step-header {
    cursor: pointer;
}

.step-tools {
    color: var(--subtext);
}

.step.failed .step-tools {
    color: var(--danger);
}

.step-thinking-body {
    margin-top: 6px;
    margin-left: 7px;
    padding-left: 12px;
    border-left: 1px solid var(--border);
}

.step-thinking-body .item,
.step-thinking-body .reasoning {
    margin-bottom: 8px;
}

.step-thinking-body > :last-child {
    margin-bottom: 0;
}

.item.tool,
.review-comment {
    background: var(--card);
    color: var(--card-text);
    border: 1px solid var(--border);
    border-radius: 5px;
    overflow: hidden;
}

.item.tool.success {
    border-color: var(--success);
}

.item.tool.error,
.item.tool.invalid,
.item.tool.aborted {
    border-color: var(--danger);
}

.tool-header,
.review-header {
    display: flex;
    gap: 10px;
    align-items: center;
    padding: 10px;
}

/* A tool is collapsed by default, like reasoning: the header is the
   clickable summary that reveals its view and output. */
.tool-header {
    cursor: pointer;
}

.tool-name,
.tool-title,
.review-index {
    font-family: ui-monospace, monospace;
    font-size: 13px;
}

.tool-name {
    background: var(--block);
    color: var(--text);
    border-radius: 5px;
    padding: 2px 5px;
}

pre.tool-block {
    background: var(--block);
    color: var(--text);
    font-family: ui-monospace, monospace;
    font-size: 13px;
    line-height: 1.5;
    margin: 0;
    padding: 10px;
    overflow-x: auto;
    white-space: pre;
}

pre.tool-block + pre.tool-block {
    border-top: 1px solid var(--border);
}

pre.tool-block.output {
    /* Cap the output at 10 lines, scrolling the rest:
       10 × (13px × 1.5) of text, plus the 20px padding. */
    max-height: 16.5em;
    overflow-y: auto;
    scrollbar-width: thin;
    scrollbar-color: var(--border) transparent;
}

pre.tool-block.output::-webkit-scrollbar {
    width: 8px;
}

pre.tool-block.output::-webkit-scrollbar-thumb {
    background: var(--border);
    border-radius: 4px;
}

.compaction {
    color: var(--subtext);
    font-size: 13px;
    text-align: center;
}

.review-content {
    padding: 10px;
}

.diff {
    background: var(--block);
    border-top: 1px solid var(--border);
    border-bottom: 1px solid var(--border);
    font-family: ui-monospace, monospace;
    font-size: 13px;
    line-height: 1.5;
}

.diff-line {
    display: flex;
    padding: 0 10px;
}

.diff-line.added {
    background: var(--added);
}

.diff-line.deleted {
    background: var(--deleted);
}

.gutter {
    min-width: 3ch;
    padding-right: 1ch;
    color: var(--subtext);
    opacity: 0.6;
    text-align: right;
    user-select: none;
}

.sign {
    width: 1ch;
}

.diff-line.added .sign {
    color: var(--success);
}

.diff-line.deleted .sign {
    color: var(--danger);
}

.diff-line .text {
    white-space: pre;
}

/* An edit's diff: the lines flow as inline, syntax-highlighted
   text, so the line is not a flex row. */
.diff.edit .diff-line {
    display: block;
    white-space: pre;
}

.item h1,
.item h2,
.item h3,
.item h4 {
    margin: 15px 0 10px;
    line-height: 1.3;
}

.item > :first-child,
.item .bubble > :first-child,
.item .review-content > :first-child {
    margin-top: 0;
}

.item > :last-child,
.item .bubble > :last-child,
.item .review-content > :last-child {
    margin-bottom: 0;
}

.item p {
    margin: 10px 0;
}

.item ul,
.item ol {
    margin: 10px 0;
    padding-left: 20px;
}

.item a {
    color: var(--link);
}

.item blockquote {
    margin: 10px 0;
    padding-left: 10px;
    border-left: 3px solid var(--border);
    color: var(--subtext);
}

.item code {
    background: var(--bubble);
    border-radius: 4px;
    padding: 1px 4px;
    font-family: ui-monospace, monospace;
    font-size: 0.9em;
}

.item pre {
    background: var(--block);
    border-radius: 5px;
    padding: 10px;
    overflow-x: auto;
}

.item pre code {
    background: none;
    padding: 0;
}

.item hr {
    margin: 15px 0;
    border: none;
    border-top: 1px solid var(--border);
}

.item table {
    margin: 10px 0;
    border-collapse: collapse;
}

.item th,
.item td {
    padding: 4px 8px;
    border: 1px solid var(--border);
}

.item input[type="checkbox"] {
    margin-right: 5px;
}
"#;

/// The script of the document. Tools are collapsed by default, so a
/// tool's outputs are anchored at their end, as a terminal would, the
/// moment the tool is opened: the newest lines are the ones that
/// matter.
const SCRIPT: &str = r#"
function anchorOutputs(scope) {
    for (const block of (scope ?? document).querySelectorAll('pre.tool-block.output')) {
        block.scrollTop = block.scrollHeight;
    }
}

for (const tool of document.querySelectorAll('details.item.tool')) {
    tool.addEventListener('toggle', () => {
        if (tool.open) {
            anchorOutputs(tool);
        }
    });
}
"#;

/// Exports the session as a standalone HTML document, styled
/// by `theme`.
pub fn export(session: &Session, theme: &Theme) -> String {
    let items = render_items(&session.items, theme);

    let started = jiff::Timestamp::try_from(session.started_at)
        .ok()
        .map(|started| {
            started
                .to_zoned(jiff::tz::TimeZone::system())
                .strftime("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_default();

    let count = session.items.len();
    let count = if count == 1 {
        "1 item".to_owned()
    } else {
        format!("{count} items")
    };

    format!(
        "<!doctype html>\n\
         <html lang=\"en\">\n\
         <head>\n\
         <meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <title>Pick session</title>\n\
         <style>\n{style}\n</style>\n\
         </head>\n\
         <body>\n\
         <main class=\"session\">\n\
         <header class=\"session-header\">{started} · {count} · Pick {version}</header>\n\
         {items}\n\
         </main>\n\
         <script>{script}</script>\n\
         </body>\n\
         </html>\n",
        style = style(theme),
        version = env!("CARGO_PKG_VERSION"),
        script = SCRIPT.trim(),
    )
}

/// The `<style>` block of the document: the `:root` variables
/// generated from the theme, followed by the static styles.
fn style(theme: &Theme) -> String {
    let palette = theme.palette();
    let seed = theme.seed();

    // The app shades its tool and diff blocks darker than the
    // page; do the same from the theme's background. The diff
    // line tints are computed the way the diff view computes
    // them: the accent, darkened, mixed into the block's
    // background.
    let block = seed.background.mix(Color::BLACK, 0.5);
    let added = palette::darken(seed.success, 0.3).mix(block, 0.97);
    let deleted = palette::darken(seed.danger, 0.3).mix(block, 0.97);

    format!(
        ":root {{
            --background: {background};
            --text: {text};
            --subtext: {subtext};
            --reasoning: {reasoning};
            --bubble: {bubble};
            --bubble-text: {bubble_text};
            --card: {card};
            --card-text: {card_text};
            --border: {border};
            --block: {block};
            --added: {added};
            --deleted: {deleted};
            --success: {success};
            --danger: {danger};
            --link: {link};
        }}
{CSS}",
        background = hex(palette.background.base.color),
        text = hex(palette.background.base.text),
        subtext = hex(palette.secondary.base.color),
        reasoning = hex(palette.secondary.strong.color),
        bubble = hex(palette.background.weak.color),
        bubble_text = hex(palette.background.weak.text),
        card = hex(palette.background.weakest.color),
        card_text = hex(palette.background.weakest.text),
        border = hex(palette.background.weak.color),
        block = hex(block),
        added = hex(added),
        deleted = hex(deleted),
        success = hex(seed.success),
        danger = hex(seed.danger),
        link = hex(seed.primary),
    )
}

/// Renders the session's items. Each assistant turn — an assistant
/// item that calls tools, followed by the tool runs it triggered —
/// is grouped into a single collapsible step; everything else (user
/// messages, a final answer, compactions, reviews) renders on its
/// own.
fn render_items(items: &[session::Item], theme: &Theme) -> String {
    let mut html = String::new();
    let mut i = 0;

    while i < items.len() {
        match &items[i] {
            session::Item::Assistant(reply) if !reply.tool_calls.is_empty() => {
                // A step is this turn plus any following message-less
                // turns (assistants with tool calls but no content),
                // merged into one. A message turn starts its own
                // step, but the message-less turns after it fold into
                // it.
                let mut turns: Vec<(&session::Reply, &[session::Item])> = Vec::new();
                let mut j = i;
                while let session::Item::Assistant(reply_j) = &items[j] {
                    let mut end = j + 1;
                    while end < items.len() && matches!(&items[end], session::Item::Tool(_)) {
                        end += 1;
                    }
                    turns.push((reply_j, &items[j + 1..end]));

                    // Keep merging while the next item is a
                    // message-less assistant with tool calls: it has
                    // no message of its own, so it folds into this
                    // step.
                    if matches!(
                        items.get(end),
                        Some(session::Item::Assistant(next))
                            if !next.tool_calls.is_empty() && next.content.is_empty()
                    ) {
                        j = end;
                    } else {
                        break;
                    }
                }

                // The first turn's message, if any, is shown on its own,
                // before the step it introduces.
                if !turns[0].0.content.is_empty() {
                    html.push_str(&render_message(&turns[0].0.content, theme));
                }

                html.push_str(&render_step(&turns, theme));
                i = j + 1 + turns.last().unwrap().1.len();
            }
            item => {
                html.push_str(&render_item(item, theme));
                i += 1;
            }
        }
    }

    html
}

/// The content message of a turn, shown on its own as an assistant
/// message, before the step it introduces. Its reasoning (if any)
/// belongs to the step, so it is not shown here.
fn render_message(content: &str, theme: &Theme) -> String {
    format!(
        "<div class=\"item assistant\">{}</div>",
        markdown(content, theme)
    )
}

/// Renders one or more merged assistant turns as a step: the header
/// shows a summary of the tools the turns called, and the turns'
/// reasoning and tool runs sit behind it. A turn's message, if any, is
/// shown on its own, before the step, not here. The step is marked
/// when one of its tools failed.
fn render_step(turns: &[(&session::Reply, &[session::Item])], theme: &Theme) -> String {
    // Every tool name across all the turns, in call order.
    let names = turns
        .iter()
        .flat_map(|(_, tools)| tools.iter())
        .filter_map(|item| {
            if let session::Item::Tool(run) = item {
                Some(run.call.name.as_str())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    // The step is marked when any of its tools failed.
    let failed = turns.iter().any(|(_, tools)| {
        tools.iter().any(|item| match item {
            session::Item::Tool(run) => matches!(
                run.status,
                session::Status::Error { .. } | session::Status::Invalid
            ),
            _ => false,
        })
    });

    // Each turn's reasoning and tool runs, revealed by the header.
    let mut thinking = String::new();
    for &(reply, tools) in turns {
        thinking.push_str(&render_reasoning(reply, theme).unwrap_or_default());
        for item in tools {
            thinking.push_str(&render_item(item, theme));
        }
    }

    // The header summarizes the tools; the exact list is kept as a
    // tooltip.
    let header = format!(
        "<span class=\"step-tools\" title=\"{raw}\">{summary}</span>",
        raw = escape(&names.join(", ")).replace('"', "&quot;"),
        summary = summarize_tools(&names)
    );

    // Revealed by the header when there is any; otherwise the header
    // is a plain label.
    let head = if thinking.is_empty() {
        format!("<div class=\"step-header\">{header}</div>")
    } else {
        format!(
            "<details class=\"step-thinking\"><summary class=\"step-header\">{header}</summary><div class=\"step-thinking-body\">{thinking}</div></details>"
        )
    };

    format!(
        "<div class=\"step{failed}\">{head}</div>",
        failed = if failed { " failed" } else { "" }
    )
}

/// A human-readable summary of the tools called in a step, e.g.
/// "Ran 3 commands, edited 4 files", counting each tool in order of
/// its first call.
fn summarize_tools(names: &[&str]) -> String {
    // The counts, in order of first call.
    let mut counts: Vec<(&str, usize)> = Vec::new();
    for name in names {
        match counts.iter_mut().find(|(found, _)| *found == *name) {
            Some((_, count)) => *count += 1,
            None => counts.push((name, 1)),
        }
    }

    counts
        .iter()
        .enumerate()
        .map(|(i, (name, count))| {
            let phrase = match *name {
                "bash" => format!("ran {} command{}", count, plural(*count)),
                "read" => format!("read {} file{}", count, plural(*count)),
                "write" => format!("wrote {} file{}", count, plural(*count)),
                "edit" => format!("edited {} file{}", count, plural(*count)),
                other => format!("called {} {} time{}", other, count, plural(*count)),
            };
            // The first phrase starts the sentence.
            if i == 0 {
                let mut chars = phrase.chars();
                match chars.next() {
                    Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                    None => phrase,
                }
            } else {
                phrase
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// The plural suffix for a count: nothing for one, "s" otherwise.
fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

/// The reasoning of a reply as a collapsible block, if it has any.
fn render_reasoning(reply: &session::Reply, theme: &Theme) -> Option<String> {
    if reply.reasoning.is_empty() {
        return None;
    }

    let summary = match reply.timings {
        Some(timings) => format!("Thought for {}", item::duration(timings.reasoning)),
        None => "Thought".to_owned(),
    };

    Some(format!(
        "<details class=\"reasoning\"><summary>{summary}</summary><div>{}</div></details>",
        markdown(&reply.reasoning, theme)
    ))
}

/// Renders one tool run as a collapsible block: the badge and title
/// are the summary, the view and output are the body. When `open` is
/// set the block starts expanded, so its content is shown.
/// Renders one tool run as a collapsible block: the badge and title
/// are the summary, the view and output are the body.
fn render_tool(run: &session::ToolRun, theme: &Theme) -> String {
    let status = match &run.status {
        session::Status::Success { .. } => "success",
        session::Status::Error { .. } => "error",
        session::Status::Invalid => "invalid",
        session::Status::Aborted => "aborted",
    };

    let title = title_of(&run.call)
        .map(|title| format!("<span class=\"tool-title\">{}</span>", escape(&title)))
        .unwrap_or_default();

    let view = view_of(&run.call, theme);

    let output = match &run.status {
        session::Status::Success { output } => {
            let output = output.to_string();

            (!output.trim().is_empty())
                .then(|| format!("<pre class=\"tool-block output\">{}</pre>", escape(&output)))
        }
        session::Status::Error { output } => Some(format!(
            "<pre class=\"tool-block output\">{}</pre>",
            escape(output)
        )),
        session::Status::Invalid => {
            Some("<pre class=\"tool-block output\">[invalid tool call]</pre>".to_owned())
        }
        session::Status::Aborted => {
            Some("<pre class=\"tool-block output\">[execution aborted]</pre>".to_owned())
        }
    };

    format!(
        "<details class=\"item tool {status}\"><summary class=\"tool-header\"><span class=\"tool-name\">{name}</span>{title}</summary>{view}{output}</details>",
        name = escape(&run.call.name),
        output = output.unwrap_or_default()
    )
}

fn render_item(item: &session::Item, theme: &Theme) -> String {
    match item {
        session::Item::User(content) => {
            format!(
                "<div class=\"item user\"><div class=\"bubble\">{}</div></div>",
                markdown(content, theme)
            )
        }
        session::Item::Assistant(reply) => {
            let content = if reply.content.is_empty() {
                String::new()
            } else {
                markdown(&reply.content, theme)
            };

            format!(
                "<div class=\"item assistant\">{reasoning}{content}</div>",
                reasoning = render_reasoning(reply, theme).unwrap_or_default()
            )
        }
        session::Item::Tool(run) => render_tool(run, theme),
        session::Item::Compaction(compaction) => format!(
            "<div class=\"item compaction\">Compacted into {} tokens</div>",
            locale::thousands(compaction.tokens)
        ),
        session::Item::Review(review) => {
            let message = review
                .message
                .as_deref()
                .map(|message| format!("<div class=\"bubble\">{}</div>", markdown(message, theme)))
                .unwrap_or_default();

            let comments = review
                .comments
                .iter()
                .map(|comment| {
                    format!(
                        "<div class=\"review-comment\"><div class=\"review-header\"><span class=\"tool-name\">review</span><span class=\"review-index\">{}</span></div><div class=\"diff\">{}</div><div class=\"review-content\">{}</div></div>",
                        escape(&format!("{}:{}", comment.path, comment.number)),
                        hunk(&comment.hunk),
                        markdown(&comment.content, theme)
                    )
                })
                .collect::<String>();

            format!("<div class=\"item review\">{message}{comments}</div>")
        }
    }
}

/// The title of a tool call, when its arguments carry one: a
/// `title`, or a `path`, or the first line of a `command`.
fn title_of(call: &reason::tool::Call) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(&call.arguments).ok()?;

    let title = value
        .get("title")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|title| !title.is_empty());

    title
        .or_else(|| value.get("path").and_then(|value| value.as_str()))
        .or_else(|| {
            value
                .get("command")
                .and_then(|value| value.as_str())
                .and_then(|command| command.lines().next())
        })
        .map(str::to_owned)
}

/// The view of a tool call's arguments, mirroring the app's
/// `Call::view`: the command of a `bash`, the content of a `write`,
/// the diff of an `edit` — and nothing for a `read`, whose title
/// carries its path. A call that cannot be parsed, or a tool without
/// a view, falls back to its raw arguments.
fn view_of(call: &reason::tool::Call, theme: &Theme) -> String {
    let value = match serde_json::from_str::<serde_json::Value>(&call.arguments) {
        Ok(value) => value,
        Err(_) => return arguments_block(&call.arguments, theme),
    };

    match call.name.as_str() {
        // A read carries no body; its title is its path.
        "read" => String::new(),
        // A tool without a view keeps its raw arguments.
        name => view(name, &value).unwrap_or_else(|| arguments_block(&call.arguments, theme)),
    }
}

/// The per-tool view of the arguments, when the tool has one.
fn view(name: &str, value: &serde_json::Value) -> Option<String> {
    match name {
        "bash" => bash_view(value),
        "write" => write_view(value),
        "edit" => edit_view(value),
        _ => None,
    }
}

/// The view of a `bash` call: the command, highlighted, with a `$`
/// prompt on its first line, as the app shows it.
fn bash_view(value: &serde_json::Value) -> Option<String> {
    let command = value.get("command")?.as_str()?;

    // The line budget matches the bash tool's preview width.
    let preview = highlight::Preview::new("bash", command, 500);

    Some(preview_block(&preview, Some("$ ")))
}

/// The view of a `write` call: the content of the file, highlighted
/// by its language, as the app shows it.
fn write_view(value: &serde_json::Value) -> Option<String> {
    let path = value.get("path")?.as_str()?;
    let content = value.get("content")?.as_str()?;

    Some(preview_block(
        &highlight::Preview::file(path, content),
        None,
    ))
}

/// The view of an `edit` call: the diff of its old and new strings,
/// as the app shows it.
fn edit_view(value: &serde_json::Value) -> Option<String> {
    let path = value.get("path")?.as_str()?;
    let old = value.get("old_string")?.as_str()?;
    let new = value.get("new_string")?.as_str()?;

    let diff = diff::Diff::new(path, old, new, tool::BACKGROUND);

    let lines = diff
        .lines
        .iter()
        .map(|line| {
            let class = match line.tag() {
                diff::Tag::Context => "context",
                diff::Tag::Addition => "added",
                diff::Tag::Deletion => "deleted",
            };

            let spans: String = line.spans_data().iter().map(span_html).collect();

            format!("<div class=\"diff-line {class}\">{spans}</div>")
        })
        .collect::<String>();

    Some(format!("<div class=\"diff edit\">{lines}</div>"))
}

/// Renders the lines of a `highlight::Preview` as a monospace block,
/// each span colored by the theme, with `prompt` prefixed to the
/// first line when present and the preview's notice as a dim line.
fn preview_block(preview: &highlight::Preview, prompt: Option<&str>) -> String {
    let lines = preview
        .lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            let prompt = (i == 0)
                .then_some(prompt)
                .flatten()
                .map(escape)
                .unwrap_or_default();

            let spans: String = line.iter().map(span_html).collect();

            format!("{prompt}{spans}")
        })
        .collect::<Vec<_>>()
        .join("\n");

    let notice = preview
        .notice
        .as_ref()
        .map(|notice| {
            format!(
                "\n<span style=\"color:var(--subtext);\">{}</span>",
                escape(notice)
            )
        })
        .unwrap_or_default();

    format!("<pre class=\"tool-block view\">{lines}{notice}</pre>")
}

/// The HTML of a syntax-highlighted span, colored by the theme.
fn span_html(span: &iced::widget::text::Span<'static>) -> String {
    let text = escape(span.text.as_ref());

    match span.color {
        Some(color) => format!("<span style=\"color:{};\">{text}</span>", hex(color)),
        None => text,
    }
}

/// The raw arguments of a call, pretty-printed and highlighted: the
/// fallback for a tool without a dedicated view.
fn arguments_block(arguments: &str, theme: &Theme) -> String {
    format!(
        "<pre class=\"tool-block arguments\"><code>{}</code></pre>",
        highlighted("json", &arguments_of(arguments), theme)
    )
}

/// The arguments of a call, pretty-printed when they are JSON.
fn arguments_of(arguments: &str) -> String {
    match serde_json::from_str::<serde_json::Value>(arguments) {
        Ok(value) => serde_json::to_string_pretty(&value).expect("a JSON value serializes"),
        Err(_) => arguments.to_owned(),
    }
}

/// Renders the lines of a hunk as diff rows.
fn hunk(hunk: &git::Hunk) -> String {
    hunk.lines
        .iter()
        .map(|line| match line {
            git::Line::Context { old, new, text } => {
                row("context", "", Some(*old), Some(*new), text)
            }
            git::Line::Added { new, text } => row("added", "+", None, Some(*new), text),
            git::Line::Deleted { old, text } => row("deleted", "-", Some(*old), None, text),
        })
        .collect()
}

fn row(class: &str, sign: &str, old: Option<usize>, new: Option<usize>, text: &str) -> String {
    format!(
        "<div class=\"diff-line {class}\"><span class=\"gutter\">{}</span><span class=\"gutter\">{}</span><span class=\"sign\">{sign}</span><span class=\"text\">{}</span></div>",
        old.map_or_else(String::new, |number| number.to_string()),
        new.map_or_else(String::new, |number| number.to_string()),
        escape(text)
    )
}

/// Renders `markdown` as HTML, highlighting the code blocks.
fn markdown(markdown: &str, theme: &Theme) -> String {
    use pulldown_cmark::{Options, Parser, html};

    // The same extensions the app's markdown widget enables.
    let options = Options::ENABLE_YAML_STYLE_METADATA_BLOCKS
        | Options::ENABLE_PLUSES_DELIMITED_METADATA_BLOCKS
        | Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS;

    // `push_html` passes raw HTML through, so prose like
    // `Range<usize>` would become a tag; escape such `<` in the
    // source first, leaving code regions to `push_html`.
    let source = escape_raw_html(markdown);

    let mut document = String::new();

    html::push_html(&mut document, Parser::new_ext(&source, options));

    highlight_code_blocks(document, theme)
}

/// Escapes the `<` of would-be raw HTML tags in the text of
/// `markdown`, so that prose like `Range<usize>` renders literally
/// instead of becoming a tag. Code regions — fenced blocks and
/// inline code spans — are left untouched, as `push_html` escapes
/// them on its own.
fn escape_raw_html(markdown: &str) -> String {
    /// The character that starts a raw HTML tag after the `<`: a
    /// letter, or `/`, `!`, or `?`.
    fn tag_like(c: Option<&char>) -> bool {
        matches!(c, Some(c) if c.is_ascii_alphabetic() || matches!(c, '/' | '!' | '?'))
    }

    let mut out = String::with_capacity(markdown.len());
    let mut fence: Option<(char, usize)> = None;

    for line in markdown.lines() {
        let trimmed = line.trim_start();

        if let Some((fence_char, fence_len)) = fence {
            // Inside a fence: the line is untouched; a line of only
            // the fence character, at least as long as the opener,
            // closes the block.
            let closes = trimmed.len() >= fence_len && trimmed.chars().all(|c| c == fence_char);
            out.push_str(line);
            out.push('\n');
            if closes {
                fence = None;
            }
            continue;
        }

        // A fence opener: a run of at least three backticks or
        // tildes at the start of the line (up to three spaces of
        // indentation).
        let indented = line.starts_with(' ') && line.len() - trimmed.len() > 3;
        let mut fence_run = 0;
        let mut fence_char = '\0';
        if !indented {
            for c in trimmed.chars() {
                if c == '`' || c == '~' {
                    if fence_char == '\0' {
                        fence_char = c;
                    } else if c != fence_char {
                        break;
                    }
                    fence_run += 1;
                } else {
                    break;
                }
            }
        }
        if fence_run >= 3 && fence_char != '\0' {
            fence = Some((fence_char, fence_run));
            out.push_str(line);
            out.push('\n');
            continue;
        }

        // A text line: walk it, tracking inline code spans, which
        // never cross a line.
        let mut code: Vec<usize> = Vec::new();
        let mut chars = line.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '`' {
                let mut run = 1;
                while chars.peek() == Some(&'`') {
                    chars.next();
                    run += 1;
                }
                if let Some(open) = code.iter().position(|&open| open == run) {
                    code.remove(open);
                } else {
                    code.push(run);
                }
                for _ in 0..run {
                    out.push('`');
                }
                continue;
            }
            if code.is_empty() && c == '<' && tag_like(chars.peek()) {
                out.push_str("&lt;");
                continue;
            }
            out.push(c);
        }
        out.push('\n');
    }

    out
}

const CODE_OPEN: &str = "<pre><code";
const CODE_CLOSE: &str = "</code></pre>";

/// Replaces the escaped content of each code block with
/// syntax-highlighted spans, leaving the blocks without a
/// language untouched.
fn highlight_code_blocks(document: String, theme: &Theme) -> String {
    let mut highlighted = String::with_capacity(document.len());
    let mut rest = document.as_str();

    while let Some(start) = rest.find(CODE_OPEN) {
        highlighted.push_str(&rest[..start]);

        let end = rest[start..]
            .find(CODE_CLOSE)
            .map(|end| start + end + CODE_CLOSE.len());

        let Some(end) = end else {
            break;
        };

        let block = &rest[start..end];
        let block = highlight_code_block(block, theme);

        highlighted.push_str(&block);

        rest = &rest[end..];
    }

    highlighted.push_str(rest);

    highlighted
}

/// Highlights the content of a `<pre><code …>…</code></pre>`
/// block, or returns it as-is when it carries no language.
fn highlight_code_block(block: &str, theme: &Theme) -> String {
    // The tag is `<pre><code>`, or `<pre><code
    // class="language-token">`; the `>` of `<pre>` is not the
    // tag's terminator, so the search starts at `<code` — the
    // language is escaped, so it cannot contain a `>`.
    let code = block.find("<code").expect("the tag is well-formed");
    let open = code + block[code..].find('>').expect("the tag is well-formed") + 1;
    let close = block.len() - CODE_CLOSE.len();

    let language = block[..open]
        .strip_prefix("<pre><code class=\"language-")
        .and_then(|rest| rest.strip_suffix('>'))
        .and_then(|rest| rest.strip_suffix('"'))
        .map(str::to_owned);

    let Some(language) = language else {
        return block.to_owned();
    };

    format!(
        "<pre><code class=\"language-{language}\">{}</code></pre>",
        highlighted(&language, &unescape(&block[open..close]), theme)
    )
}

/// Reverses the escaping `push_html` applies to body text:
/// `&`, `<` and `>`.
fn unescape(source: &str) -> String {
    // `&amp;` last, so that an escaped `&lt;` does not become `<`.
    let mut source = source.to_owned();

    source = source.replace("&gt;", ">");
    source = source.replace("&lt;", "<");
    source = source.replace("&amp;", "&");

    source
}

/// Highlights `source` as `language`, returning HTML spans
/// styled by the theme.
fn highlighted(language: &str, source: &str, theme: &Theme) -> String {
    let mut parser = highlighter::Parser::new(&highlighter::Settings {
        token: language.to_owned(),
    });

    source
        .lines()
        .map(|line| {
            parser
                .parse_line(line)
                .map(|(range, code)| {
                    let text = escape(&line[range]);
                    let style = code.highlight(theme);

                    match (style.color, style.style) {
                        (Some(color), Some(font::Style::Italic | font::Style::Oblique)) => format!(
                            "<span style=\"color:{}; font-style:italic;\">{text}</span>",
                            hex(color)
                        ),
                        (Some(color), _) => {
                            format!("<span style=\"color:{};\">{text}</span>", hex(color))
                        }
                        (None, _) => text,
                    }
                })
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Escapes `text` for inclusion in HTML.
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// The CSS color of an iced color.
fn hex(color: Color) -> String {
    let [r, g, b, _] = color.into_rgba8();

    format!("#{r:02x}{g:02x}{b:02x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::core::Output;
    use std::time::{Duration, SystemTime};

    fn export(items: Vec<session::Item>) -> String {
        super::export(
            &Session {
                version: session::Version::current(),
                started_at: SystemTime::UNIX_EPOCH,
                items,
            },
            &Theme::CatppuccinMocha,
        )
    }

    #[test]
    fn an_empty_session_exports_a_document() {
        let document = export(vec![]);

        assert!(document.starts_with("<!doctype html>"));
        assert!(document.contains("<title>Pick session</title>"));
        assert!(document.contains("0 items · Pick "));
    }

    #[test]
    fn the_css_is_styled_by_the_theme() {
        let document = export(vec![]);

        // The Catppuccin Mocha seed colors, verbatim.
        assert!(document.contains("--background: #1e1e2e"));
        assert!(document.contains("--success: #a6e3a1"));
        assert!(document.contains("--danger: #f38ba8"));
        assert!(document.contains("--link: #89b4fa"));
    }

    #[test]
    fn user_messages_are_rendered_as_markdown() {
        let document = export(vec![session::Item::User(
            "hello **world**, a & b < c".to_owned(),
        )]);

        assert!(document.contains("<div class=\"item user\">"));
        assert!(document.contains("<strong>world</strong>"));
        assert!(document.contains("a &amp; b &lt; c"));
    }

    #[test]
    fn text_that_looks_like_html_is_rendered_literally() {
        // Rust generics in prose would otherwise be passed through as
        // raw HTML tags.
        let document = export(vec![session::Item::Assistant(session::Reply {
            prompt: Default::default(),
            reasoning: String::new(),
            content: "items are (Range<usize>, Code) and Box<dyn Iterator>".to_owned(),
            tool_calls: vec![],
            timings: None,
        })]);

        assert!(document.contains("Range&lt;usize&gt;"));
        assert!(document.contains("Box&lt;dyn Iterator&gt;"));
        assert!(!document.contains("<usize>"));
    }

    #[test]
    fn code_regions_keep_their_angle_brackets() {
        // Fenced blocks and inline code spans are left to `push_html`,
        // so their `<` is escaped exactly once, never twice.
        let document = export(vec![session::Item::User(
            "inline `Vec<usize>` and a block:\n```rust\nlet v: Vec<usize> = vec![];\n```\n"
                .to_owned(),
        )]);

        assert!(document.contains("<code>Vec&lt;usize&gt;</code>"));
        assert!(document.contains("language-rust"));
        assert!(document.contains("&lt;usize&gt;"));
        assert!(!document.contains("&amp;lt;"));
    }

    #[test]
    fn assistant_reasoning_is_collapsed_with_its_duration() {
        let document = export(vec![session::Item::Assistant(session::Reply {
            prompt: Default::default(),
            reasoning: "thinking".to_owned(),
            content: "it is **done**".to_owned(),
            tool_calls: vec![],
            timings: Some(reason::Timings {
                reasoning: Duration::from_millis(50),
                ..Default::default()
            }),
        })]);

        assert!(document.contains("<div class=\"item assistant\">"));
        assert!(
            document
                .contains("<details class=\"reasoning\"><summary>Thought for 50ms</summary><div>")
        );
        assert!(document.contains("<strong>done</strong>"));
    }

    #[test]
    fn a_turn_groups_its_reasoning_and_tools_into_a_step() {
        let call = reason::tool::Call {
            id: "call_1".to_owned().into(),
            name: "bash".to_owned(),
            arguments: r#"{"command":"git status","title":"Check the status"}"#.to_owned(),
        };

        let document = export(vec![
            session::Item::Assistant(session::Reply {
                prompt: Default::default(),
                reasoning: "let me check the status".to_owned(),
                content: String::new(),
                tool_calls: vec![call.clone()],
                timings: Some(reason::Timings {
                    reasoning: Duration::from_millis(50),
                    ..Default::default()
                }),
            }),
            session::Item::Tool(session::ToolRun {
                call,
                status: session::Status::Success {
                    output: Output::new(),
                },
            }),
            // The final answer is rendered on its own.
            session::Item::Assistant(session::Reply {
                prompt: Default::default(),
                reasoning: String::new(),
                content: "All good.".to_owned(),
                tool_calls: vec![],
                timings: None,
            }),
        ]);

        // The turn is a step, numbered and labelled with the tool it
        // called; its reasoning and tool run sit behind the header.
        assert!(document.contains("<div class=\"step\">"));
        assert!(
            document.contains("<span class=\"step-tools\" title=\"bash\">Ran 1 command</span>")
        );

        // The tool run and reasoning are behind the header, inside the
        // step, which carries no message of its own.
        let step = &document[document.find("<div class=\"step\">").unwrap()
            ..document.find("<div class=\"item assistant\">").unwrap()];
        assert!(step.contains("item tool success"));
        assert!(!step.contains(" open"));
        assert!(step.contains("step-thinking"));
        assert!(step.contains("let me check the status"));
        assert!(step.contains("Thought for 50ms"));
        assert!(!step.contains("step-content"));

        // The final answer is a plain assistant, rendered on its own.
        assert!(document.contains("<div class=\"item assistant\"><p>All good.</p>"));
    }

    #[test]
    fn consecutive_messageless_turns_are_merged_into_one_step() {
        let bash1 = reason::tool::Call {
            id: "c1".to_owned().into(),
            name: "bash".to_owned(),
            arguments: r#"{"command":"make"}"#.to_owned(),
        };
        let read1 = reason::tool::Call {
            id: "c2".to_owned().into(),
            name: "read".to_owned(),
            arguments: r#"{"path":"a.rs"}"#.to_owned(),
        };
        let bash2 = reason::tool::Call {
            id: "c3".to_owned().into(),
            name: "bash".to_owned(),
            arguments: r#"{"command":"test"}"#.to_owned(),
        };

        let document = export(vec![
            // Turn 1: reasoning + one bash call, no message.
            session::Item::Assistant(session::Reply {
                prompt: Default::default(),
                reasoning: "build it".to_owned(),
                content: String::new(),
                tool_calls: vec![bash1.clone()],
                timings: None,
            }),
            session::Item::Tool(session::ToolRun {
                call: bash1,
                status: session::Status::Success {
                    output: Output::new(),
                },
            }),
            // Turn 2: reasoning + a read and a bash call, no message.
            session::Item::Assistant(session::Reply {
                prompt: Default::default(),
                reasoning: "inspect the result".to_owned(),
                content: String::new(),
                tool_calls: vec![read1.clone(), bash2.clone()],
                timings: None,
            }),
            session::Item::Tool(session::ToolRun {
                call: read1,
                status: session::Status::Success {
                    output: Output::new(),
                },
            }),
            session::Item::Tool(session::ToolRun {
                call: bash2,
                status: session::Status::Success {
                    output: Output::new(),
                },
            }),
        ]);

        // The two message-less turns are merged into a single step.
        assert_eq!(document.matches("class=\"step-header\"").count(), 1);
        // The header summarizes every tool across the turns, with the
        // exact list in the tooltip.
        assert!(document.contains(
            "<span class=\"step-tools\" title=\"bash, read, bash\">Ran 2 commands, read 1 file</span>"
        ));

        // Both turns' reasoning and all three tool runs sit behind the
        // single header.
        assert!(document.contains("build it"));
        assert!(document.contains("inspect the result"));
        assert_eq!(document.matches("item tool success").count(), 3);
    }

    #[test]
    fn a_message_turn_breaks_the_merge() {
        let bash1 = reason::tool::Call {
            id: "c1".to_owned().into(),
            name: "bash".to_owned(),
            arguments: r#"{"command":"make"}"#.to_owned(),
        };
        let read1 = reason::tool::Call {
            id: "c2".to_owned().into(),
            name: "read".to_owned(),
            arguments: r#"{"path":"a.rs"}"#.to_owned(),
        };

        let document = export(vec![
            // Turn 1: a message-less action turn.
            session::Item::Assistant(session::Reply {
                prompt: Default::default(),
                reasoning: String::new(),
                content: String::new(),
                tool_calls: vec![bash1.clone()],
                timings: None,
            }),
            session::Item::Tool(session::ToolRun {
                call: bash1,
                status: session::Status::Success {
                    output: Output::new(),
                },
            }),
            // Turn 2: a message turn; it stands on its own.
            session::Item::Assistant(session::Reply {
                prompt: Default::default(),
                reasoning: String::new(),
                content: "Now I will read it.".to_owned(),
                tool_calls: vec![read1.clone()],
                timings: None,
            }),
            session::Item::Tool(session::ToolRun {
                call: read1,
                status: session::Status::Success {
                    output: Output::new(),
                },
            }),
        ]);

        // The message turn is not merged into the action turn: there
        // are two steps.
        assert_eq!(document.matches("class=\"step-header\"").count(), 2);

        // The message turn's message is shown on its own, before its
        // step; each step keeps its own tools.
        let message = document
            .find("<div class=\"item assistant\"><p>Now I will read it.</p>")
            .unwrap();
        let step2 = document.rfind("class=\"step-header\"").unwrap();
        assert!(message < step2);
        assert!(
            document.contains("<span class=\"step-tools\" title=\"bash\">Ran 1 command</span>")
        );
        assert!(document.contains("<span class=\"step-tools\" title=\"read\">Read 1 file</span>"));
    }

    #[test]
    fn a_messageless_turn_folds_into_the_preceding_step() {
        // A message-less turn after a message turn folds into the
        // message turn's step, so no message-less step stands alone
        // right after it.
        let bash1 = reason::tool::Call {
            id: "c1".to_owned().into(),
            name: "bash".to_owned(),
            arguments: r#"{"command":"make"}"#.to_owned(),
        };
        let read1 = reason::tool::Call {
            id: "c2".to_owned().into(),
            name: "read".to_owned(),
            arguments: r#"{"path":"a.rs"}"#.to_owned(),
        };

        let document = export(vec![
            // Turn 1: a message turn.
            session::Item::Assistant(session::Reply {
                prompt: Default::default(),
                reasoning: String::new(),
                content: "Build it, then look at the result.".to_owned(),
                tool_calls: vec![bash1.clone()],
                timings: None,
            }),
            session::Item::Tool(session::ToolRun {
                call: bash1,
                status: session::Status::Success {
                    output: Output::new(),
                },
            }),
            // Turn 2: a message-less turn; it folds into the step
            // above.
            session::Item::Assistant(session::Reply {
                prompt: Default::default(),
                reasoning: String::new(),
                content: String::new(),
                tool_calls: vec![read1.clone()],
                timings: None,
            }),
            session::Item::Tool(session::ToolRun {
                call: read1,
                status: session::Status::Success {
                    output: Output::new(),
                },
            }),
        ]);

        // One step, with the message turn's message on its own,
        // before it.
        assert_eq!(document.matches("class=\"step-header\"").count(), 1);
        let message = document
            .find("<div class=\"item assistant\"><p>Build it, then look at the result.</p>")
            .unwrap();
        let step = document.find("class=\"step-header\"").unwrap();
        assert!(message < step);

        // The step carries both turns' tools and runs.
        assert!(document.contains(
            "<span class=\"step-tools\" title=\"bash, read\">Ran 1 command, read 1 file</span>"
        ));
        let step_html = &document[step..document.find("</main>").unwrap()];
        assert_eq!(step_html.matches("item tool success").count(), 2);
    }

    #[test]
    fn the_step_header_summarizes_its_tools() {
        let call = |id: &str, name: &str| reason::tool::Call {
            id: id.to_owned().into(),
            name: name.to_owned(),
            arguments: r#"{"command":"true"}"#.to_owned(),
        };
        let run = |id: &str, name: &str| {
            session::Item::Tool(session::ToolRun {
                call: call(id, name),
                status: session::Status::Success {
                    output: Output::new(),
                },
            })
        };

        // bash, bash, edit, bash, edit.
        let document = export(vec![
            session::Item::Assistant(session::Reply {
                prompt: Default::default(),
                reasoning: String::new(),
                content: String::new(),
                tool_calls: vec![
                    call("c1", "bash"),
                    call("c2", "bash"),
                    call("c3", "edit"),
                    call("c4", "bash"),
                    call("c5", "edit"),
                ],
                timings: None,
            }),
            run("c1", "bash"),
            run("c2", "bash"),
            run("c3", "edit"),
            run("c4", "bash"),
            run("c5", "edit"),
        ]);

        // The header summarizes the calls, counting each tool in
        // order of its first call, with the exact list in the
        // tooltip.
        assert!(document.contains(
            "<span class=\"step-tools\" title=\"bash, bash, edit, bash, edit\">Ran 3 commands, edited 2 files</span>"
        ));
    }

    #[test]
    fn an_unknown_tool_is_summarized_by_name() {
        let call = reason::tool::Call {
            id: "c1".to_owned().into(),
            name: "grep".to_owned(),
            arguments: r#"{"pattern":"x"}"#.to_owned(),
        };

        let document = export(vec![
            session::Item::Assistant(session::Reply {
                prompt: Default::default(),
                reasoning: String::new(),
                content: String::new(),
                tool_calls: vec![call.clone(), call.clone()],
                timings: None,
            }),
            session::Item::Tool(session::ToolRun {
                call: call.clone(),
                status: session::Status::Success {
                    output: Output::new(),
                },
            }),
            session::Item::Tool(session::ToolRun {
                call,
                status: session::Status::Success {
                    output: Output::new(),
                },
            }),
        ]);

        assert!(document.contains(
            "<span class=\"step-tools\" title=\"grep, grep\">Called grep 2 times</span>"
        ));
    }

    #[test]
    fn a_turn_carries_its_message_and_the_answer_is_separate() {
        // A turn carries its own message (content) together with its
        // tool calls; the final answer is a separate assistant,
        // rendered on its own.
        let document = export(vec![
            session::Item::Assistant(session::Reply {
                prompt: Default::default(),
                reasoning: String::new(),
                content: "Let me check the locking and the layout.".to_owned(),
                tool_calls: vec![reason::tool::Call {
                    id: "c1".to_owned().into(),
                    name: "read".to_owned(),
                    arguments: r#"{"path":"file.rs"}"#.to_owned(),
                }],
                timings: None,
            }),
            session::Item::Tool(session::ToolRun {
                call: reason::tool::Call {
                    id: "c1".to_owned().into(),
                    name: "read".to_owned(),
                    arguments: r#"{"path":"file.rs"}"#.to_owned(),
                },
                status: session::Status::Success {
                    output: Output::new(),
                },
            }),
            session::Item::Assistant(session::Reply {
                prompt: Default::default(),
                reasoning: String::new(),
                content: "Yes, it is workable.".to_owned(),
                tool_calls: vec![],
                timings: None,
            }),
        ]);

        // The turn's message is shown on its own, before the step it
        // introduces; the step carries the tools, not the message.
        let message = document
            .find("<div class=\"item assistant\"><p>Let me check the locking and the layout.</p>")
            .unwrap();
        let step = document.find("<div class=\"step\">").unwrap();
        let answer = document
            .find("<div class=\"item assistant\"><p>Yes, it is workable.</p>")
            .unwrap();
        assert!(message < step);
        assert!(step < answer);
        assert!(document.contains("<span class=\"step-tools\" title=\"read\">Read 1 file</span>"));

        // The step carries the tool run, not the message.
        let step_html = &document[step..answer];
        assert!(step_html.contains("item tool success"));
        assert!(!step_html.contains("Let me check the locking and the layout."));

        // The final answer is a plain assistant, rendered on its own.
        assert!(document.contains("<div class=\"item assistant\"><p>Yes, it is workable.</p>"));
    }

    #[test]
    fn a_failed_tool_marks_its_step() {
        let document = export(vec![
            session::Item::Assistant(session::Reply {
                prompt: Default::default(),
                reasoning: String::new(),
                content: String::new(),
                tool_calls: vec![reason::tool::Call {
                    id: "call_1".to_owned().into(),
                    name: "bash".to_owned(),
                    arguments: r#"{"command":"false"}"#.to_owned(),
                }],
                timings: None,
            }),
            session::Item::Tool(session::ToolRun {
                call: reason::tool::Call {
                    id: "call_1".to_owned().into(),
                    name: "bash".to_owned(),
                    arguments: r#"{"command":"false"}"#.to_owned(),
                },
                status: session::Status::Error {
                    output: "boom".to_owned(),
                },
            }),
        ]);

        // The step carries the `failed` marker; its (only) tool run
        // sits behind the header and no content message is shown.
        assert!(document.contains("<div class=\"step failed\">"));
        assert!(document.contains("item tool error"));
        assert!(!document.contains(" open"));
        assert!(document.contains("boom"));
    }

    #[test]
    fn a_content_message_is_shown_before_its_step() {
        let mut first = Output::new();
        first.push("built".to_owned());
        let mut last = Output::new();
        last.push("a b c".to_owned());

        let document = export(vec![
            session::Item::Assistant(session::Reply {
                prompt: Default::default(),
                reasoning: "build it, then look".to_owned(),
                content: "All fixed.".to_owned(),
                tool_calls: vec![
                    reason::tool::Call {
                        id: "c1".to_owned().into(),
                        name: "bash".to_owned(),
                        arguments: r#"{"command":"make"}"#.to_owned(),
                    },
                    reason::tool::Call {
                        id: "c2".to_owned().into(),
                        name: "edit".to_owned(),
                        arguments: r#"{"path":"a.rs","old_string":"x","new_string":"y"}"#
                            .to_owned(),
                    },
                ],
                timings: None,
            }),
            session::Item::Tool(session::ToolRun {
                call: reason::tool::Call {
                    id: "c1".to_owned().into(),
                    name: "bash".to_owned(),
                    arguments: r#"{"command":"make"}"#.to_owned(),
                },
                status: session::Status::Success { output: first },
            }),
            session::Item::Tool(session::ToolRun {
                call: reason::tool::Call {
                    id: "c2".to_owned().into(),
                    name: "edit".to_owned(),
                    arguments: r#"{"path":"a.rs","old_string":"x","new_string":"y"}"#.to_owned(),
                },
                status: session::Status::Success { output: last },
            }),
        ]);

        // The turn's message is shown on its own, before the step it
        // introduces.
        let message = document
            .find("<div class=\"item assistant\"><p>All fixed.</p>")
            .unwrap();
        let step = document.find("<div class=\"step\">").unwrap();
        assert!(message < step);

        // The step names both tools and carries the reasoning and tool
        // runs, but not the message.
        let step_html = &document[step..document.find("</main>").unwrap()];
        assert!(step_html.contains(
            "<span class=\"step-tools\" title=\"bash, edit\">Ran 1 command, edited 1 file</span>"
        ));
        assert!(step_html.contains("build it, then look"));
        assert!(step_html.contains("item tool success"));
        assert!(!step_html.contains("All fixed."));
    }

    #[test]
    fn fenced_code_blocks_are_highlighted() {
        let code = "```rust\nlet x = 1;\n```\n";

        let document = export(vec![session::Item::User(code.to_owned())]);

        let open = document
            .find("class=\"language-rust\"")
            .expect("the block keeps its language");

        let block = &document[open..document[open..].find("</code>").unwrap() + open];

        // The keyword is highlighted with the theme's primary
        // color, the constant with the danger color.
        assert!(block.contains("style=\"color:#89b4fa;\">let</span>"));
        assert!(block.contains("style=\"color:#f38ba8;\">1</span>"));
    }

    #[test]
    fn a_bash_tool_shows_its_command_and_output() {
        let mut output = Output::new();
        output.push("line 1".to_owned());
        output.push("line 2".to_owned());

        let document = export(vec![session::Item::Tool(session::ToolRun {
            call: reason::tool::Call {
                id: "call_1".to_owned().into(),
                name: "bash".to_owned(),
                arguments: r#"{"command":"git status","title":"Check the status"}"#.to_owned(),
            },
            status: session::Status::Success { output },
        })]);

        assert!(document.contains("<details class=\"item tool success\">"));
        assert!(document.contains("<span class=\"tool-title\">Check the status</span>"));

        // The view is the command with a `$` prompt, not the raw JSON.
        assert!(document.contains("tool-block view\">$ "));
        assert!(document.contains("status"));
        assert!(!document.contains("tool-block arguments"));

        // The output is rendered in full.
        assert!(document.contains("<pre class=\"tool-block output\">line 1\nline 2</pre>"));
    }

    #[test]
    fn an_edit_tool_shows_its_diff() {
        let document = export(vec![session::Item::Tool(session::ToolRun {
            call: reason::tool::Call {
                id: "call_1".to_owned().into(),
                name: "edit".to_owned(),
                arguments: r#"{"path":"a.rs","old_string":"let x = 1;","new_string":"let x = 2;"}"#
                    .to_owned(),
            },
            status: session::Status::Success {
                output: Output::new(),
            },
        })]);

        assert!(document.contains("<span class=\"tool-title\">a.rs</span>"));

        // The view is the diff of the old and new strings.
        assert!(document.contains("diff edit"));
        assert!(document.contains("diff-line deleted"));
        assert!(document.contains("diff-line added"));
        assert!(document.contains("1;"));
        assert!(document.contains("2;"));
        assert!(!document.contains("tool-block arguments"));
    }

    #[test]
    fn a_write_tool_shows_its_content() {
        let document = export(vec![session::Item::Tool(session::ToolRun {
            call: reason::tool::Call {
                id: "call_1".to_owned().into(),
                name: "write".to_owned(),
                arguments: r#"{"path":"a.rs","content":"let s = \"hi\";"}"#.to_owned(),
            },
            status: session::Status::Success {
                output: Output::new(),
            },
        })]);

        assert!(document.contains("<span class=\"tool-title\">a.rs</span>"));

        // The view is the file's content, highlighted.
        assert!(document.contains("tool-block view"));
        assert!(document.contains("let"));
        assert!(document.contains("hi"));
        assert!(!document.contains("tool-block arguments"));
    }

    #[test]
    fn a_read_tool_shows_no_body_and_an_unknown_tool_keeps_its_arguments() {
        let read = export(vec![session::Item::Tool(session::ToolRun {
            call: reason::tool::Call {
                id: "call_1".to_owned().into(),
                name: "read".to_owned(),
                arguments: r#"{"path":"a.rs","offset":3,"limit":10}"#.to_owned(),
            },
            status: session::Status::Success {
                output: Output::new(),
            },
        })]);

        // A read's title carries its path; it has no view body.
        assert!(read.contains("<span class=\"tool-title\">a.rs</span>"));
        let body = &read[read.find("<main").unwrap()..read.find("</main>").unwrap()];
        assert!(!body.contains("tool-block"));
        assert!(!body.contains("diff-line"));

        let mystery = export(vec![session::Item::Tool(session::ToolRun {
            call: reason::tool::Call {
                id: "call_1".to_owned().into(),
                name: "mystery".to_owned(),
                arguments: r#"{"foo":"bar"}"#.to_owned(),
            },
            status: session::Status::Success {
                output: Output::new(),
            },
        })]);

        // A tool without a view falls back to its raw arguments.
        let body = &mystery[mystery.find("<main").unwrap()..mystery.find("</main>").unwrap()];
        assert!(body.contains("tool-block arguments"));
        assert!(body.contains("foo"));
    }

    #[test]
    fn tool_outputs_are_capped_and_anchored_at_their_end() {
        let mut output = Output::new();

        for line in 0..20 {
            output.push(format!("line {line}"));
        }

        let document = export(vec![session::Item::Tool(session::ToolRun {
            call: reason::tool::Call {
                id: "call_1".to_owned().into(),
                name: "bash".to_owned(),
                arguments: "{}".to_owned(),
            },
            status: session::Status::Success { output },
        })]);

        // The output block is capped at 10 lines and scrolls.
        assert!(document.contains("max-height: 16.5em"));
        assert!(document.contains("overflow-y: auto"));
        assert!(document.contains("<pre class=\"tool-block output\">"));

        // On load, the outputs are anchored at their end.
        assert!(document.contains("block.scrollTop = block.scrollHeight"));
    }

    #[test]
    fn a_tool_error_renders_its_message() {
        let document = export(vec![session::Item::Tool(session::ToolRun {
            call: reason::tool::Call {
                id: "call_1".to_owned().into(),
                name: "bash".to_owned(),
                arguments: r#"{"command":"false"}"#.to_owned(),
            },
            status: session::Status::Error {
                output: "boom & bap < 0".to_owned(),
            },
        })]);

        assert!(document.contains("<details class=\"item tool error\">"));
        assert!(document.contains("boom &amp; bap &lt; 0"));
    }

    #[test]
    fn a_review_renders_its_message_and_hunks() {
        let hunk = git::Hunk {
            old: git::Range { start: 1, count: 3 },
            new: git::Range { start: 1, count: 2 },
            heading: None,
            lines: std::sync::Arc::from([
                git::Line::Context {
                    old: 1,
                    new: 1,
                    text: "let a = 1;".to_owned(),
                },
                git::Line::Added {
                    new: 2,
                    text: "let b = 2;".to_owned(),
                },
                git::Line::Deleted {
                    old: 3,
                    text: "let c = 3;".to_owned(),
                },
            ]),
        };

        let document = export(vec![session::Item::Review(session::Review {
            message: Some("fix these".to_owned()),
            comments: vec![session::Comment {
                path: "src/main.rs".to_owned(),
                number: git::Number::New(2),
                hunk,
                content: "check this".to_owned(),
            }],
        })]);

        assert!(document.contains("<div class=\"item review\">"));
        assert!(document.contains("src/main.rs:R2"));
        assert!(document.contains("<div class=\"diff-line context\">"));
        assert!(document.contains("<div class=\"diff-line added\">"));
        assert!(document.contains("<div class=\"diff-line deleted\">"));
        assert!(document.contains("let b = 2;"));
        assert!(document.contains("<p>check this</p>"));
    }

    #[test]
    fn a_compaction_renders_its_token_count() {
        let document = export(vec![session::Item::Compaction(session::Compaction {
            reply: session::Reply::default(),
            tokens: 1_234_567,
            reasoning_tokens: 100,
            to: 3,
        })]);

        assert!(document.contains("Compacted into 1,234,567 tokens"));
    }
}
