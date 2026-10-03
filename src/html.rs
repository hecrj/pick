//! Exports a session as a standalone HTML document, styled by
//! the app's theme.

use crate::core::{Project, Session, git, session};
use crate::{diff, font, highlight, item, locale, tool};
use iced::highlighter;
use iced::theme::palette;
use iced::{Color, Theme, time};
use std::ops::Range;

/// The static styles of the document, colored by the `:root`
/// variables generated from the theme.
const CSS: &str = r#"
/* Every scrollbar — the page's, a code block's, a tool
   output's — keeps its default size but gets a
   border-colored thumb on a transparent track. */
* {
    box-sizing: border-box;
    scrollbar-color: var(--border) transparent;
}

body {
    margin: 0;
    background: var(--background);
    color: var(--text);
    font: 15px/1.6 system-ui, -apple-system, "Segoe UI", sans-serif;
}

/* The same style where the standard properties are not
   supported. */
::-webkit-scrollbar-track {
    background: transparent;
}

::-webkit-scrollbar-thumb {
    background: var(--border);
    border-radius: 4px;
}

.session {
    max-width: 770px;
    margin: 0 auto;
    padding: 40px 16px;
}

.session-header {
    margin-bottom: 24px;
}

.session-title {
    margin: 0 0 5px;
    color: var(--text);
    font-size: 24px;
    font-weight: bold;
}

.session-meta {
    color: var(--subtext);
    font-size: 13px;
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

/* A turn groups the work a message concludes — its reasoning
   and tool runs — behind a header that summarizes it, ruled
   off the way the app's `Turn::Work` is. */
.turn {
    margin-bottom: 20px;
}

.turn-header {
    background: var(--card);
    color: var(--reasoning);
    border: 1px solid var(--border);
    border-radius: 5px;
    padding: 10px;
    font-size: 14px;
    font-weight: bold;
    cursor: pointer;
}

.turn[open] .turn-header {
    color: var(--text);
}

/* The arrow is our own; hide the summary's native marker,
   which `list-style` reaches in Firefox and modern Chrome,
   and the legacy pseudo-element in WebKit. */
summary.turn-header,
.reasoning summary {
    list-style: none;
}

summary.turn-header::-webkit-details-marker,
.reasoning summary::-webkit-details-marker {
    display: none;
}

/* The arrow follows the collapsible's own open state:
   pointing right while it is closed, down while it is
   open. The rule reaches only the summary the arrow sits
   in, so an open turn does not turn its children's arrows
   down. */
.arrow {
    margin-left: 10px;
    font-weight: normal;
}

.arrow::before {
    content: "▸";
}

.turn[open] > summary .arrow::before,
.reasoning[open] > summary .arrow::before {
    content: "▾";
}

/* The rule starts at the title box's bottom edge and is inset
   from the left, the way the app's `Turn::Work` rules its
   items off. */
.turn-body {
    margin-left: 10px;
    padding: 10px 0 0 10px;
    border-left: 2px solid var(--border);
}

.turn-body > * {
    margin-bottom: 10px;
}

.turn-body > :last-child {
    margin-bottom: 0;
}

.reasoning {
    background: var(--card);
    color: var(--reasoning);
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

/* The 1px padding keeps the card showing through between the
   border and the view, the way the app's box padding does. */
.item.tool,
.review-comment {
    background: var(--card);
    color: var(--card-text);
    border: 1px solid var(--border);
    border-radius: 5px;
    padding: 1px;
    overflow: hidden;
}

.tool-header,
.review-header {
    display: flex;
    gap: 10px;
    align-items: center;
    padding: 10px;
}

/* A tool is collapsed by default, the way the app collapses a
   tool's work: the header is the clickable summary that reveals
   its view and output. */
.tool-header {
    cursor: pointer;
}

/* The comment's text is padded, the way the app pads its
   markdown container. */
.review-content {
    padding: 10px;
}

.tool-name,
.tool-title,
.review-index {
    font-family: ui-monospace, monospace;
    font-size: 13px;
}

.tool-name {
    background: var(--block);
    color: #fff;
    border-radius: 2px;
    padding: 2px 5px;
}

.item.tool.error .tool-title,
.item.tool.invalid .tool-title,
.item.tool.aborted .tool-title {
    color: var(--danger);
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

/* A command is a single unit of intent, not a line of a log:
   wrap it, keeping its newlines, and break a word that would
   otherwise run past the edge, instead of scrolling sideways. */
pre.tool-block.view {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
}

/* The view and the output are inset from the card's left,
   right, and bottom edges. There is no top margin: the gap
   above is the header's padding, or the view's bottom margin,
   so a top one would double it. */
.item.tool > pre.tool-block.view,
.item.tool > pre.tool-block.arguments,
.item.tool > .diff,
pre.tool-block.output {
    margin: 0 10px 10px;
}

/* A tool's output, bordered by its status, the way the app
   borders it. The export keeps the full log, so the block is
   capped and scrolls its overflow. */
pre.tool-block.output {
    border: 1px solid var(--danger);
    border-radius: 2px;

    /* 10 × (13px × 1.5) of text, plus the 20px padding. */
    max-height: 16.5em;
    overflow-y: auto;
}

pre.tool-block.output.success {
    border-color: var(--success-dim);
}

.diff {
    background: var(--block);
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
.item .review-content > :first-child,
.reasoning div > :first-child {
    margin-top: 0;
}

.item > :last-child,
.item .bubble > :last-child,
.item .review-content > :last-child,
.reasoning div > :last-child {
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

.item code,
.reasoning div code {
    background: var(--code);
    color: var(--code-text);
    border-radius: 4px;
    padding: 1px 4px;
    font-family: ui-monospace, monospace;
    font-size: 0.9em;
}

.item pre,
.reasoning div pre {
    background: var(--block);
    border-radius: 5px;
    padding: 10px;
    overflow-x: auto;
}

.item pre code,
.reasoning div pre code {
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

.compaction {
    color: var(--subtext);
    font-size: 13px;
    text-align: center;
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

/// The tally of what an export concealed: the spans of text it
/// masked, and the blocks it replaced with a redaction notice.
#[derive(Default, Debug, PartialEq, Eq)]
pub struct Redactions {
    /// The spans masked: secrets and home paths.
    pub masked: usize,
    /// The blocks replaced: the views and outputs of calls that
    /// may expose credentials.
    pub blocks: usize,
}

impl Redactions {
    /// Whether nothing was concealed.
    pub fn is_empty(&self) -> bool {
        self.masked == 0 && self.blocks == 0
    }
}

/// The context of an export being rendered: the document it
/// builds, what it renders against, and how it treats sensitive
/// content. Every byte of the document is written through its
/// writers — `tag` for an element, `text` for content, `html`
/// for trusted markup.
struct Render<'a> {
    /// The document, built in order.
    html: String,
    project: &'a Project,
    theme: &'a Theme,
    /// Whether the export is raw: it conceals nothing.
    raw: bool,
    /// The tally of what the export has concealed so far.
    redactions: Redactions,
}

impl Render<'_> {
    /// Writes an element: its start tag, the content the closure
    /// writes, and its end tag.
    fn tag<F: FnOnce(&mut Self)>(&mut self, name: &str, attrs: &str, content: F) {
        self.html.push_str(&format!("<{name}{attrs}>"));
        content(self);
        self.html.push_str(&format!("</{name}>"));
    }

    /// Writes trusted markup as-is: the document's static
    /// structure, and the output of the fragment builders, which
    /// escape and redact what they contain.
    fn html(&mut self, markup: &str) {
        self.html.push_str(markup);
    }

    /// Redacts `text`, tallying the concealment — or passes it
    /// through when the export is raw.
    fn redact(&mut self, text: &str) -> String {
        if self.raw {
            return text.to_owned();
        }

        let (text, masked) = redact(text);
        self.redactions.masked += masked;
        text
    }

    /// Writes `text`, redacted and escaped.
    fn text(&mut self, text: &str) {
        let text = escape(&self.redact(text));
        self.html.push_str(&text);
    }

    /// The notice that replaces a block that may expose
    /// credentials, tallying the concealment.
    fn conceal(&mut self) -> &'static str {
        self.redactions.blocks += 1;
        "[redacted: may contain credentials]"
    }
}

/// Exports `session` as a standalone HTML document, styled by
/// `theme`, unless `raw`, in which case the content is exported
/// verbatim. Returns the document and the tally of what it
/// concealed.
pub fn export(
    session: &Session,
    project: &Project,
    theme: &Theme,
    title: Option<&str>,
    raw: bool,
) -> (String, Redactions) {
    let mut render = Render {
        html: String::new(),
        project,
        theme,
        raw,
        redactions: Redactions::default(),
    };

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

    // The given title, or none: it headlines the session, above
    // its metadata, and names the document.
    let title = title.filter(|title| !title.is_empty());

    render.html(
        "<!doctype html>\n\
         <html lang=\"en\">\n\
         <head>\n\
         <meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n",
    );

    render.tag("title", "", |r| r.text(title.unwrap_or("Piolet session")));

    render.html(&format!(
        "\n\
         <style>\n{style}\n</style>\n\
         </head>\n\
         <body>\n\
         <main class=\"session\">\n\
         <header class=\"session-header\">",
        style = style(theme),
    ));

    if let Some(title) = title {
        render.tag("h1", " class=\"session-title\"", |r| r.text(title));
    }

    render.tag("div", " class=\"session-meta\"", |r| {
        r.text(&format!(
            "{started} · {count} · Piolet {}",
            env!("CARGO_PKG_VERSION")
        ));
    });

    render.html("</header>\n");

    items(&session.items, &mut render);

    render.html(&format!(
        "</main>\n<script>{}</script>\n</body>\n</html>\n",
        SCRIPT.trim()
    ));

    (render.html, render.redactions)
}

/// The `<style>` block of the document: the `:root` variables
/// generated from the theme, followed by the static styles.
fn style(theme: &Theme) -> String {
    let palette = theme.palette();
    let seed = theme.seed();

    // The app shades its tool and diff blocks with a fixed dark
    // background (`tool::BACKGROUND`); do the same, and compute
    // the diff line tints the way the diff view computes them:
    // the accent, darkened, mixed into the block's background.
    let block = Color::from_rgb8(0x11, 0x11, 0x11);
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
            --code: {code};
            --code-text: {code_text};
            --card: {card};
            --card-text: {card_text};
            --border: {border};
            --block: {block};
            --added: {added};
            --deleted: {deleted};
            --success: {success};
            --success-dim: {success_dim};
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
        // Inline code takes the tier one step below the
        // bubble's, the way the app's markdown widget styles
        // it.
        code = hex(palette.background.weaker.color),
        code_text = hex(palette.background.weaker.text),
        card = hex(palette.background.weakest.color),
        card_text = hex(palette.background.weakest.text),
        border = hex(palette.background.weak.color),
        block = hex(block),
        added = hex(added),
        deleted = hex(deleted),
        success = hex(seed.success),
        // The app dims a successful tool's output border by
        // halving its alpha.
        success_dim = rgba(seed.success.scale_alpha(0.5)),
        danger = hex(seed.danger),
        link = hex(seed.primary),
    )
}
/// Renders the session's items the way the app renders its
/// `Turn`s: a user message, review, or compaction stands on
/// its own; every other run — message-less replies and tool
/// runs — groups into a turn, the way the app's `Turn::Work`
/// does.
fn items(items: &[session::Item], render: &mut Render<'_>) {
    let mut i = 0;

    while i < items.len() {
        let start = i;

        loop {
            let current = &items[i];
            i += 1;

            match current {
                session::Item::User(_)
                | session::Item::Review(_)
                | session::Item::Compaction(_) => {
                    item(current, render);
                    break;
                }
                session::Item::Assistant(reply) if !reply.content.is_empty() => {
                    // The message concludes the turn the run
                    // it belongs to makes.
                    turn(&items[start..i], i == items.len(), render);
                    break;
                }
                _ => {
                    match items.get(i) {
                        None
                        | Some(
                            session::Item::User(_)
                            | session::Item::Review(_)
                            | session::Item::Compaction(_),
                        ) => {
                            // The run ends at a standalone item,
                            // or with the session; the turn is
                            // in flight when the session ends.
                            turn(&items[start..i], i == items.len(), render);
                            break;
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

/// Renders a turn, the way the app renders a `Turn::Work`: a
/// bordered header summarizes the work, the work itself sits
/// behind it, and the message that concludes it, if any,
/// follows on its own. A turn of a single item inlines the
/// item instead, as its own header already carries the work.
/// The turn that ends a session with no concluding message
/// starts open, as the app forces its in-flight turn open.
fn turn(turn: &[session::Item], ends_session: bool, render: &mut Render<'_>) {
    // The message that concludes the turn, if any.
    let reply = match turn.last() {
        Some(session::Item::Assistant(reply)) if !reply.content.is_empty() => Some(reply),
        _ => None,
    };

    let open = ends_session && reply.is_none();

    // Every reply's reasoning and tool run, revealed by the
    // header; a single item needs no group of its own, as it
    // stands in for the turn.
    let mut body = 0;

    for item in turn {
        match item {
            session::Item::Assistant(reply) if !reply.reasoning.is_empty() => body += 1,
            session::Item::Tool(_) => body += 1,
            _ => {}
        }
    }

    if body != 1 {
        let attrs = if open {
            " class=\"turn\" open"
        } else {
            " class=\"turn\""
        };

        render.tag("details", attrs, |r| {
            r.tag("summary", " class=\"turn-header\"", |r| {
                let summary = summary_of(turn, r.project);
                r.tag("span", " class=\"turn-summary\"", |r| r.text(&summary));
                r.tag("span", " class=\"arrow\"", |_| {});
            });
            r.tag("div", " class=\"turn-body\"", |r| {
                for item in turn {
                    match item {
                        session::Item::Assistant(reply) => reasoning(reply, r),
                        session::Item::Tool(run) => tool(run, r),
                        _ => {}
                    }
                }
            });
        });
    } else {
        for item in turn {
            match item {
                session::Item::Assistant(reply) => reasoning(reply, render),
                session::Item::Tool(run) => tool(run, render),
                _ => {}
            }
        }
    }

    if let Some(reply) = reply {
        let content = markdown(&reply.content, render);
        render.tag("div", " class=\"item assistant\"", |r| r.html(&content));
    }
}

/// Renders an item that stands on its own — a user message, a
/// review, or a compaction. Assistant and tool runs group
/// into turns and never stand alone.
fn item(item: &session::Item, render: &mut Render<'_>) {
    match item {
        session::Item::User(content) => {
            let content = markdown(content, render);
            render.tag("div", " class=\"item user\"", |r| {
                r.tag("div", " class=\"bubble\"", |r| r.html(&content));
            });
        }
        session::Item::Compaction(compaction) => {
            render.tag("div", " class=\"item compaction\"", |r| {
                r.text(&format!(
                    "Compacted into {} tokens",
                    locale::thousands(compaction.tokens)
                ));
            });
        }
        session::Item::Review(review) => {
            render.tag("div", " class=\"item review\"", |r| {
                if let Some(message) = &review.message {
                    let message = markdown(message, r);
                    r.tag("div", " class=\"bubble\"", |r| r.html(&message));
                }

                for comment in &review.comments {
                    r.tag("div", " class=\"review-comment\"", |r| {
                        r.tag("div", " class=\"review-header\"", |r| {
                            r.tag("span", " class=\"tool-name\"", |r| r.text("review"));
                            r.tag("span", " class=\"review-index\"", |r| {
                                r.text(&format!("{}:{}", comment.path, comment.number));
                            });
                        });

                        let hunk = hunk(&comment.hunk, r);
                        let content = markdown(&comment.content, r);
                        r.tag("div", " class=\"diff\"", |r| r.html(&hunk));
                        r.tag("div", " class=\"review-content\"", |r| r.html(&content));
                    });
                }
            });
        }
        session::Item::Assistant(_) | session::Item::Tool(_) => {
            // Assistant and tool runs group into turns; they
            // are rendered by `turn`, never on their own.
        }
    }
}

/// Renders one reply's reasoning as a collapsed block, if it
/// has any: the header carries its duration, the way the
/// app's compact reasoning does.
fn reasoning(reply: &session::Reply, render: &mut Render<'_>) {
    if reply.reasoning.is_empty() {
        return;
    }

    let is_done = !reply.content.is_empty() || !reply.tool_calls.is_empty();

    let summary = match reply.timings {
        Some(timings) => {
            if is_done {
                format!("Thought for {}", item::duration(timings.reasoning))
            } else {
                format!("Thinking... ({})", item::duration(timings.reasoning))
            }
        }
        None => {
            if is_done {
                "Thought".to_owned()
            } else {
                "Thinking...".to_owned()
            }
        }
    };

    let reasoning = markdown(&reply.reasoning, render);

    // The `item` class gives the box its margin, which it
    // needs once a turn inlines it.
    render.tag("details", " class=\"item reasoning\"", |r| {
        r.tag("summary", "", |r| {
            r.tag("span", "", |r| r.text(&summary));
            r.tag("span", " class=\"arrow\"", |_| {});
        });
        r.tag("div", "", |r| r.html(&reasoning));
    });
}

/// Renders one tool run as a collapsed block, the way the
/// app renders a tool: the badge and title are the summary,
/// and the view and the output — bordered by its status —
/// sit behind it. A run that may expose credentials conceals
/// the block it would leak: an `env`-listing bash or a
/// credentials read its output, a credentials write or edit
/// its view.
fn tool(run: &session::ToolRun, render: &mut Render<'_>) {
    let status = match &run.status {
        session::Status::Success { .. } => "success",
        session::Status::Error { .. } => "error",
        session::Status::Invalid => "invalid",
        session::Status::Aborted => "aborted",
    };

    let name = run.call.name.as_str();

    let sensitive = tool::Tool::builtins()
        .get(name)
        .and_then(|tool| tool.parse(&run.call.arguments).ok())
        .is_some_and(|state| state.is_sensitive(render.project));

    render.tag("details", &format!(" class=\"item tool {status}\""), |r| {
        r.tag("summary", " class=\"tool-header\"", |r| {
            r.tag("span", " class=\"tool-name\"", |r| r.text(&run.call.name));

            if let Some(title) = title_of(run, r.project) {
                r.tag("span", " class=\"tool-title\"", |r| r.text(&title));
            }
        });

        if !r.raw && sensitive && matches!(name, "write" | "edit") {
            let notice = r.conceal();
            r.tag("pre", " class=\"tool-block view\"", |r| {
                r.tag("em", "", |r| r.text(notice));
            });
        } else {
            let view = view_of(&run.call, r);
            r.html(&view);
        }

        if !r.raw && sensitive && matches!(name, "bash" | "read") {
            let notice = r.conceal();
            r.tag(
                "pre",
                &format!(" class=\"tool-block output {status}\""),
                |r| {
                    r.tag("em", "", |r| r.text(notice));
                },
            );
        } else {
            match &run.status {
                // The export keeps the full log; the block is
                // capped and scrolls its overflow.
                session::Status::Success { output } if output.lines() > 0 => {
                    r.tag("pre", " class=\"tool-block output success\"", |r| {
                        r.text(&output.all().collect::<Vec<_>>().join("\n"));
                    });
                }
                // An empty output renders nothing, the way the
                // app renders it.
                session::Status::Success { .. } => {}
                session::Status::Error { output } => {
                    let output = output.trim();
                    let output = if output.is_empty() {
                        "[No output]"
                    } else {
                        output
                    };
                    r.tag(
                        "pre",
                        &format!(" class=\"tool-block output {status}\""),
                        |r| r.text(output),
                    );
                }
                session::Status::Invalid => {
                    r.tag("pre", " class=\"tool-block output invalid\"", |r| {
                        r.text("[invalid tool call]")
                    });
                }
                session::Status::Aborted => {
                    r.tag("pre", " class=\"tool-block output aborted\"", |r| {
                        r.text("[execution aborted]")
                    });
                }
            }
        }
    });
}

/// Renders the lines of a hunk as diff rows.
fn hunk(hunk: &git::Hunk, render: &mut Render<'_>) -> String {
    hunk.lines
        .iter()
        .map(|line| match line {
            git::Line::Context { old, new, text } => {
                row("context", "", Some(*old), Some(*new), text, render)
            }
            git::Line::Added { new, text } => row("added", "+", None, Some(*new), text, render),
            git::Line::Deleted { old, text } => row("deleted", "-", Some(*old), None, text, render),
        })
        .collect()
}

fn row(
    class: &str,
    sign: &str,
    old: Option<usize>,
    new: Option<usize>,
    text: &str,
    render: &mut Render<'_>,
) -> String {
    format!(
        "<div class=\"diff-line {class}\"><span class=\"gutter\">{}</span><span class=\"gutter\">{}</span><span class=\"sign\">{sign}</span><span class=\"text\">{}</span></div>",
        old.map_or_else(String::new, |number| number.to_string()),
        new.map_or_else(String::new, |number| number.to_string()),
        escape(&render.redact(text))
    )
}

/// Renders `markdown` as HTML, highlighting the code blocks.
fn markdown(markdown: &str, render: &mut Render<'_>) -> String {
    use pulldown_cmark::{Options, Parser, html};

    // The same extensions the app's markdown widget enables.
    let options = Options::ENABLE_YAML_STYLE_METADATA_BLOCKS
        | Options::ENABLE_PLUSES_DELIMITED_METADATA_BLOCKS
        | Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS;

    let markdown = render.redact(markdown);

    // `push_html` passes raw HTML through, so prose like
    // `Range<usize>` would become a tag; escape such `<` in the
    // source first, leaving code regions to `push_html`.
    let source = escape_raw_html(&markdown);

    let mut document = String::new();

    html::push_html(&mut document, Parser::new_ext(&source, options));

    highlight_code_blocks(document, render.theme)
}

/// The summary of a turn's work, the way the app summarizes a
/// `Turn::Work`: the single command's title, or the counts of
/// the commands, reads, edits, and writes the turn ran, and how
/// long its replies took to think.
fn summary_of(turn: &[session::Item], project: &Project) -> String {
    let mut reasoning = time::Duration::ZERO;
    let mut command = String::new();
    let mut commands = 0;
    let mut reads = 0;
    let mut edits = 0;
    let mut writes = 0;

    for item in turn {
        match item {
            session::Item::Assistant(reply) => {
                if let Some(timings) = reply.timings {
                    reasoning += timings.reasoning;
                }
            }
            session::Item::Tool(run) => match run.call.name.as_str() {
                "bash" => {
                    if command.is_empty() {
                        command = title_of(run, project).unwrap_or_default();
                    }

                    commands += 1
                }
                "read" => reads += 1,
                "edit" => edits += 1,
                "write" => writes += 1,
                _ => {}
            },
            _ => {}
        }
    }

    let inflect = |word: &str, count: usize| {
        if count == 1 {
            word.to_owned()
        } else {
            format!("{word}s")
        }
    };

    let mut summary = [
        (commands > 0).then(|| {
            if commands == 1 && !command.is_empty() {
                command
            } else {
                format!("ran {commands} {}", inflect("command", commands))
            }
        }),
        (reads > 0).then(|| format!("read {reads} {}", inflect("file", reads))),
        (edits > 0).then(|| format!("edited {edits} {}", inflect("file", edits))),
        (writes > 0).then(|| format!("wrote {writes} {}", inflect("file", writes))),
        (reasoning > time::Duration::ZERO)
            .then(|| format!("thought for {}", item::duration(reasoning))),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(", ");

    if summary.is_empty() {
        summary = "Catching up...".to_owned();
    }

    capitalize(&summary)
}

/// Capitalizes the first character of `text`, the way the app
/// capitalizes a turn's summary.
fn capitalize(text: &str) -> String {
    let mut chars = text.chars();

    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// The title of a tool run, the way the app titles it: the
/// tool's parsed state titles it — a bash's `description`, a
/// read, write, or edit's project-relative path. The title is
/// unredacted: the writer redacts it.
fn title_of(run: &session::ToolRun, project: &Project) -> Option<String> {
    let tools = tool::Tool::builtins();
    let state = tools
        .get(run.call.name.as_str())?
        .parse(&run.call.arguments)
        .ok()?;

    state.title(project).map(|title| title.into_owned())
}

/// The view of a tool call's arguments, mirroring the app's
/// `Call::view`: the command of a `bash`, the content of a
/// `write`, the diff of an `edit` — and nothing for a `read`,
/// whose title carries its path. A call that cannot be parsed,
/// or a tool without a view, falls back to its raw arguments.
fn view_of(call: &reason::tool::Call, render: &mut Render<'_>) -> String {
    let value = match serde_json::from_str::<serde_json::Value>(&call.arguments) {
        Ok(value) => value,
        Err(_) => return arguments_block(&call.arguments, render),
    };

    match call.name.as_str() {
        // A read carries no body; its title is its path.
        "read" => String::new(),
        // A tool without a view keeps its raw arguments.
        name => {
            view(name, &value, render).unwrap_or_else(|| arguments_block(&call.arguments, render))
        }
    }
}

/// The per-tool view of the arguments, when the tool has one.
fn view(name: &str, value: &serde_json::Value, render: &mut Render<'_>) -> Option<String> {
    match name {
        "bash" => bash_view(value, render),
        "write" => write_view(value, render),
        "edit" => edit_view(value, render),
        _ => None,
    }
}

/// The view of a `bash` call: the command, highlighted, with a `$`
/// prompt on its first line, as the app shows it.
fn bash_view(value: &serde_json::Value, render: &mut Render<'_>) -> Option<String> {
    let command = value.get("command")?.as_str()?;
    let command = render.redact(command);

    // The line budget matches the bash tool's preview width.
    let preview = highlight::Preview::new("bash", &command, 500);

    Some(preview_block(&preview, Some("$ ")))
}

/// The view of a `write` call: the content of the file, highlighted
/// by its language, as the app shows it.
fn write_view(value: &serde_json::Value, render: &mut Render<'_>) -> Option<String> {
    let path = value.get("path")?.as_str()?;
    let content = value.get("content")?.as_str()?;
    let content = render.redact(content);

    Some(preview_block(
        &highlight::Preview::file(path, &content),
        None,
    ))
}

/// The view of an `edit` call: the diff of its old and new strings,
/// as the app shows it.
fn edit_view(value: &serde_json::Value, render: &mut Render<'_>) -> Option<String> {
    let path = value.get("path")?.as_str()?;
    let old = value.get("old_string")?.as_str()?;
    let new = value.get("new_string")?.as_str()?;
    let old = render.redact(old);
    let new = render.redact(new);

    let diff = diff::Diff::new(path, &old, &new, tool::BACKGROUND);

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
/// fallback for a tool without a view.
fn arguments_block(arguments: &str, render: &mut Render<'_>) -> String {
    format!(
        "<pre class=\"tool-block arguments\"><code>{}</code></pre>",
        highlighted(
            "json",
            &arguments_of(&render.redact(arguments)),
            render.theme
        )
    )
}

/// The arguments of a call, pretty-printed when they are JSON.
fn arguments_of(arguments: &str) -> String {
    match serde_json::from_str::<serde_json::Value>(arguments) {
        Ok(value) => serde_json::to_string_pretty(&value).expect("a JSON value serializes"),
        Err(_) => arguments.to_owned(),
    }
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

/// A masked value.
const MASKED: &str = "•••";

/// The triggers of a secret-ish name: a name is secret-ish when
/// it contains one of these as a segment.
const TRIGGERS: &[&str] = &["key", "token", "secret", "pass", "pwd", "credential"];

/// Common words that contain a trigger but are not secret
/// names.
const STOPWORDS: &[&str] = &[
    "compass",
    "donkey",
    "hockey",
    "monkey",
    "passage",
    "sneaky",
    "tokenize",
    "tokenizer",
    "turkey",
    "whiskey",
];

/// Masks the secrets and abbreviates the home directory in
/// `text`, returning the masked text and the number of spans
/// concealed.
fn redact(text: &str) -> (String, usize) {
    let (text, masked) = mask_secrets(text);
    let (text, paths) = abbreviate_home(&text);

    (text, masked + paths)
}

/// Masks the secrets in `text`, returning the masked text and
/// the number of spans concealed.
fn mask_secrets(text: &str) -> (String, usize) {
    let mut masked = 0;
    let mut lines = String::with_capacity(text.len());

    for line in text.split_inclusive('\n') {
        let (line, spans) = mask_line(line);
        masked += spans;
        lines.push_str(&line);
    }

    let (lines, blocks) = mask_private_key_blocks(&lines);

    (lines, masked + blocks)
}

/// Masks the secrets on one line, returning the masked line and
/// the number of spans concealed.
fn mask_line(line: &str) -> (String, usize) {
    // The line's content, without its ending newline.
    let end = match line.strip_suffix('\n') {
        Some(content) => content.len(),
        None => line.len(),
    };

    let mut spans = Vec::new();

    mask_headers(line, end, &mut spans);
    mask_assignments(line, end, &mut spans);
    mask_bearer(line, end, &mut spans);
    mask_url_credentials(line, end, &mut spans);
    mask_tokens(line, end, &mut spans);

    replace_spans(line, spans)
}

/// Masks the value of an `Authorization` header.
fn mask_headers(line: &str, end: usize, spans: &mut Vec<Range<usize>>) {
    let lower = line[..end].to_lowercase();

    if let Some((index, name)) = lower.match_indices("authorization:").next() {
        spans.push(index + name.len()..end);
    }
}

/// Masks the values of secret-ish assignments — `API_KEY=…`,
/// `token: …` — leaving the names and separators.
fn mask_assignments(line: &str, end: usize, spans: &mut Vec<Range<usize>>) {
    for (index, separator) in line[..end].match_indices(['=', ':']) {
        let separator = separator
            .chars()
            .next()
            .expect("a matched separator is a character");
        let name = name_before(line, index);

        if !is_secret_name(name) {
            continue;
        }

        let value = &line[index + 1..end];

        if let Some(span) = value_span(value, separator) {
            spans.push(index + 1 + span.start..index + 1 + span.end);
        }
    }
}

/// The run of identifier characters ending at `index` in `line`.
fn name_before(line: &str, index: usize) -> &str {
    let mut start = index;

    for (i, c) in line[..index].char_indices().rev() {
        if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
            start = i;
        } else {
            break;
        }
    }

    &line[start..index]
}

/// Whether `name` looks like the name of a secret: a plausible
/// identifier of at least four characters, not a common word,
/// containing a trigger as a segment.
fn is_secret_name(name: &str) -> bool {
    if name.len() < 4
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    {
        return false;
    }

    if STOPWORDS.contains(&name.to_ascii_lowercase().as_str()) {
        return false;
    }

    let lower = name.to_ascii_lowercase();

    TRIGGERS.iter().any(|trigger| {
        lower
            .match_indices(trigger)
            .any(|(index, _)| trigger_boundary(name, index))
    })
}

/// Whether the trigger at `index` opens a segment of `name`: at
/// the start, after a non-letter, or across a case change —
/// `apiKey` and `APIKEY` yes, `monkey` no.
fn trigger_boundary(name: &str, index: usize) -> bool {
    if index == 0 {
        return true;
    }

    let previous = name.as_bytes()[index - 1];

    if !previous.is_ascii_alphabetic() {
        return true;
    }

    // A lowercase letter never opens a segment mid-word.
    name.as_bytes()[index].is_ascii_uppercase()
}

/// The span, in `value`, of the part of a value worth masking:
/// an empty or indirect value — a variable reference, a
/// placeholder — is not.
fn value_span(value: &str, separator: char) -> Option<Range<usize>> {
    let offset = value.len() - value.trim_start().len();
    let value = &value[offset..];

    if value.is_empty() || value.starts_with(['$', '<']) {
        return None;
    }

    match value.as_bytes().first()? {
        quote @ (b'"' | b'\'') => {
            let rest = &value[1..];
            let finish = rest
                .find(*quote as char)
                .map(|close| close + 1)
                .unwrap_or(rest.len());
            let inner = rest[..finish.saturating_sub(1)].trim();

            if is_placeholder(inner) {
                return None;
            }

            Some(offset..offset + finish)
        }
        _ => {
            let finish = if separator == ':' {
                value.len()
            } else {
                value.find(char::is_whitespace).unwrap_or(value.len())
            };
            let inner = value[..finish].trim_end();

            if is_placeholder(inner) {
                return None;
            }

            Some(offset..offset + inner.len())
        }
    }
}

/// Whether a value names nothing: a placeholder rather than a
/// secret.
fn is_placeholder(value: &str) -> bool {
    matches!(value, "" | "null" | "none" | "true" | "false" | "-" | "…")
}

/// Masks a `Bearer` token: the run of word characters following
/// the word, when it is long enough to be one — sixteen or
/// more.
fn mask_bearer(line: &str, end: usize, spans: &mut Vec<Range<usize>>) {
    let lower = line[..end].to_lowercase();

    for (index, _) in lower.match_indices("bearer") {
        if index > 0 && line.as_bytes()[index - 1].is_ascii_alphabetic() {
            continue;
        }

        let after = index + "bearer".len();
        let rest = line[after..end].trim_start();
        let offset = line[after..end].len() - rest.len();
        let start = after + offset;
        let finish = run_end_of(line, end, start, token_chars);

        if finish - start >= 16 {
            spans.push(start..finish);
        }
    }
}

/// Masks the password of a `scheme://user:pass@host` URL.
fn mask_url_credentials(line: &str, end: usize, spans: &mut Vec<Range<usize>>) {
    for (index, _) in line[..end].match_indices("://") {
        let rest = &line[index + 3..end];
        let whitespace = rest.find(char::is_whitespace).unwrap_or(rest.len());
        let Some(at) = rest[..whitespace].find('@') else {
            continue;
        };

        let authority = &rest[..at];
        let Some(colon) = authority.find(':') else {
            continue;
        };

        if colon + 1 < at {
            spans.push(index + 3 + colon + 1..index + 3 + at);
        }
    }
}

/// Masks the known token shapes: the providers' prefixes, and
/// the JSON Web Tokens.
fn mask_tokens(line: &str, end: usize, spans: &mut Vec<Range<usize>>) {
    for prefix in ["ghp_", "gho_", "ghu_", "ghs_", "ghr_", "github_pat_"] {
        mask_prefixed(line, end, prefix, 16, None, spans, token_chars);
    }

    mask_prefixed(line, end, "sk-", 20, None, spans, token_chars);

    for prefix in ["xoxb-", "xoxp-", "xoxa-", "xoxs-"] {
        mask_prefixed(line, end, prefix, 10, None, spans, token_chars);
    }

    // An AWS access key id: `AKIA` and exactly sixteen capitals.
    mask_prefixed(line, end, "AKIA", 16, Some(16), spans, aws_chars);

    // A Google API key: `AIza` and exactly thirty-five
    // characters.
    mask_prefixed(line, end, "AIza", 35, Some(35), spans, token_chars);

    mask_jwt(line, end, spans);
}

/// Masks each occurrence of `prefix` in `line` followed by a run
/// of `is_char` characters — at least `min` long, exactly
/// `exact` long when given — when the prefix is not embedded in
/// a longer word.
fn mask_prefixed(
    line: &str,
    end: usize,
    prefix: &str,
    min: usize,
    exact: Option<usize>,
    spans: &mut Vec<Range<usize>>,
    is_char: impl Fn(char) -> bool,
) {
    let mut search_from = 0;

    while let Some(index) = line[search_from..end].find(prefix) {
        let index = search_from + index;

        if word_boundary(line, index) {
            let start = index + prefix.len();
            let finish = run_end_of(line, end, start, &is_char);

            match exact {
                Some(exact) if finish - start == exact => spans.push(index..finish),
                None if finish - start >= min => spans.push(index..finish),
                _ => {}
            }
        }

        search_from = index + 1;
    }
}

/// Whether a token may start at `index` in `line`: not embedded
/// in a longer run of word characters.
fn word_boundary(line: &str, index: usize) -> bool {
    line.as_bytes()
        .get(index.wrapping_sub(1))
        .is_none_or(|byte| !byte.is_ascii_alphanumeric() && *byte != b'_' && *byte != b'-')
}

/// The end of the run of `is_char` characters starting at
/// `start` in `line`, within `end`.
fn run_end_of(line: &str, end: usize, start: usize, is_char: impl Fn(char) -> bool) -> usize {
    let mut finish = start;

    for (i, c) in line[start..end].char_indices() {
        if is_char(c) {
            finish = start + i + c.len_utf8();
        } else {
            break;
        }
    }

    finish
}

/// The characters of a token: word characters, and the dash,
/// dot, and underscore of the providers' alphabets.
fn token_chars(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_')
}

/// The characters of an AWS access key id: capitals and digits.
fn aws_chars(c: char) -> bool {
    c.is_ascii_uppercase() || c.is_ascii_digit()
}

/// Masks a JSON Web Token: an `eyJ`-headed run of three
/// base64url segments joined by dots.
fn mask_jwt(line: &str, end: usize, spans: &mut Vec<Range<usize>>) {
    let mut search_from = 0;

    while let Some(index) = line[search_from..end].find("eyJ") {
        let index = search_from + index;

        if word_boundary(line, index)
            && let Some(finish) = jwt_finish(line, end, index)
        {
            spans.push(index..finish);
            search_from = finish;
            continue;
        }

        search_from = index + 1;
    }
}

/// The end of the JSON Web Token starting at `index` in `line`,
/// when its segments look like base64url.
fn jwt_finish(line: &str, end: usize, index: usize) -> Option<usize> {
    let base64url = |c: char| c.is_ascii_alphanumeric() || matches!(c, '-' | '_');

    let rest = &line[index..end];
    let mut segments = rest.split('.');
    let head = segments.next()?;
    let second = segments.next()?;
    let third = segments.next()?;

    if head.len() < 8 || second.len() < 8 {
        return None;
    }

    if !(head.bytes().all(|byte| base64url(byte as char))
        && second.bytes().all(|byte| base64url(byte as char)))
    {
        return None;
    }

    let third = third
        .bytes()
        .take_while(|byte| base64url(*byte as char))
        .count();

    Some(index + head.len() + 1 + second.len() + 1 + third)
}

/// Masks the bodies of private-key blocks: from a
/// `-----BEGIN … PRIVATE KEY-----` marker to its `-----END …`
/// marker, or, when the block is cut off, to the end of its
/// base64 body.
fn mask_private_key_blocks(text: &str) -> (String, usize) {
    let mut spans = Vec::new();
    let mut search_from = 0;

    while let Some(index) = text[search_from..].find("-----BEGIN") {
        let index = search_from + index;

        let line_end = text[index..]
            .find('\n')
            .map_or(text.len(), |offset| index + offset);

        if text[index..line_end].contains("PRIVATE KEY") {
            let finish = match text[line_end..].find("-----END") {
                Some(offset) => {
                    let end = line_end + offset;
                    end + text[end..].find('\n').map_or(0, |offset| offset + 1)
                }
                None => {
                    let mut finish = line_end;

                    for line in text[finish..].lines() {
                        let base64 = line.bytes().all(|byte| {
                            byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'=')
                        });

                        if line.is_empty() || !base64 {
                            break;
                        }

                        finish += line.len() + 1;
                    }

                    finish
                }
            };

            spans.push(index..finish);
            search_from = finish;
        } else {
            search_from = index + 1;
        }
    }

    replace_spans(text, spans)
}

/// Replaces the spans of `input` with `MASKED`, merging
/// overlaps, and returns the text and the number of spans
/// replaced.
fn replace_spans(input: &str, spans: Vec<Range<usize>>) -> (String, usize) {
    let mut spans: Vec<Range<usize>> = spans
        .into_iter()
        .filter(|span| span.start < span.end)
        .collect();

    if spans.is_empty() {
        return (input.to_owned(), 0);
    }

    spans.sort_by_key(|span| span.start);

    let mut merged: Vec<Range<usize>> = Vec::new();

    for span in spans {
        match merged.last_mut() {
            Some(last) if span.start <= last.end => last.end = last.end.max(span.end),
            _ => merged.push(span),
        }
    }

    let mut output = String::with_capacity(input.len());
    let mut start = 0;

    for span in &merged {
        output.push_str(&input[start..span.start]);
        output.push_str(MASKED);
        start = span.end;
    }

    output.push_str(&input[start..]);

    (output, merged.len())
}

/// Abbreviates the home directory in `text` to `~`, returning
/// the text and the number of paths abbreviated.
fn abbreviate_home(text: &str) -> (String, usize) {
    let Some(home) = std::env::home_dir() else {
        return (text.to_owned(), 0);
    };

    abbreviate_home_in(text, &home.to_string_lossy())
}

/// The home abbreviation with the home directory given, so it
/// can be tested.
fn abbreviate_home_in(text: &str, home: &str) -> (String, usize) {
    if home.is_empty() {
        return (text.to_owned(), 0);
    }

    let (text, paths) = replace_count(text, &format!("{home}/"), "~/");
    let (text, rest) = replace_count(&text, home, "~");

    (text, paths + rest)
}

/// Replaces `from` with `to` in `text`, returning the text and
/// the number of replacements.
fn replace_count(text: &str, from: &str, to: &str) -> (String, usize) {
    (text.replace(from, to), text.matches(from).count())
}

/// The CSS color of an iced color.
fn hex(color: Color) -> String {
    let [r, g, b, _] = color.into_rgba8();

    format!("#{r:02x}{g:02x}{b:02x}")
}

/// The CSS color of an iced color, keeping its alpha.
fn rgba(color: Color) -> String {
    let [r, g, b, a] = color.into_rgba8();

    format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::core::Output;
    use std::time::{Duration, SystemTime};

    fn export(items: Vec<session::Item>) -> String {
        export_all(items, None, false).0
    }

    fn export_all(
        items: Vec<session::Item>,
        title: Option<&str>,
        raw: bool,
    ) -> (String, Redactions) {
        super::export(
            &Session {
                version: session::Version::current(),
                started_at: SystemTime::UNIX_EPOCH,
                items,
            },
            &Project::current_dir().expect("a project exists"),
            &Theme::CatppuccinMocha,
            title,
            raw,
        )
    }

    fn export_titled(items: Vec<session::Item>, title: &str) -> String {
        export_all(items, Some(title), false).0
    }

    fn assistant(
        reasoning: &str,
        content: &str,
        calls: Vec<reason::tool::Call>,
        timings: Option<reason::Timings>,
    ) -> session::Item {
        session::Item::Assistant(session::Reply {
            prompt: Default::default(),
            reasoning: reasoning.to_owned(),
            content: content.to_owned(),
            tool_calls: calls,
            timings,
        })
    }

    fn call(id: &str, name: &str, arguments: &str) -> reason::tool::Call {
        reason::tool::Call {
            id: id.to_owned().into(),
            name: name.to_owned(),
            arguments: arguments.to_owned(),
        }
    }

    fn tool(id: &str, name: &str, arguments: &str) -> session::Item {
        session::Item::Tool(session::ToolRun {
            call: call(id, name, arguments),
            status: session::Status::Success {
                output: Output::new(),
            },
        })
    }

    fn timings(duration: Duration) -> Option<reason::Timings> {
        Some(reason::Timings {
            reasoning: duration,
            ..Default::default()
        })
    }

    #[test]
    fn an_empty_session_exports_a_document() {
        let document = export(vec![]);

        assert!(document.starts_with("<!doctype html>"));
        assert!(document.contains("<title>Piolet session</title>"));
        assert!(document.contains("0 items · Piolet "));
        assert!(!document.contains("<h1 class=\"session-title\">"));
    }

    #[test]
    fn a_title_headlines_the_export() {
        let document = export_titled(vec![], "Rebasing onto master");

        // The title becomes the document's title and headlines
        // the session, above its metadata.
        assert!(document.contains("<title>Rebasing onto master</title>"));
        let heading = document
            .find("<h1 class=\"session-title\">Rebasing onto master</h1>")
            .unwrap();
        let meta = document.find("<div class=\"session-meta\">").unwrap();
        assert!(heading < meta);

        // Like any other content, a title is escaped.
        let document = export_titled(vec![], "a < b & c");
        assert!(document.contains("<title>a &lt; b &amp; c</title>"));
        assert!(document.contains("<h1 class=\"session-title\">a &lt; b &amp; c</h1>"));
    }

    #[test]
    fn the_css_is_styled_by_the_theme() {
        let document = export(vec![]);

        // The Catppuccin Mocha seed colors, verbatim.
        assert!(document.contains("--background: #1e1e2e"));
        assert!(document.contains("--success: #a6e3a1"));
        assert!(document.contains("--danger: #f38ba8"));
        assert!(document.contains("--link: #89b4fa"));

        // The tool and diff blocks use the app's fixed dark
        // background.
        assert!(document.contains("--block: #111111"));

        // Inline code takes the `weaker` background tier, one
        // step darker than the bubble's `weak` tier — not the
        // bubble's own color.
        let tier = |name: &str| -> String {
            let i = document.find(name).expect("variable");
            document[i + name.len() + 2..i + name.len() + 9].to_owned()
        };
        assert_ne!(tier("--code"), tier("--bubble"));

        // The collapsibles' summaries hide their native
        // markers, so only our arrow shows, which follows the
        // open state.
        assert!(
            document
                .contains("summary.turn-header,\n.reasoning summary {\n    list-style: none;\n}")
        );
        assert!(document.contains(
            "summary.turn-header::-webkit-details-marker,\n.reasoning summary::-webkit-details-marker"
        ));
        assert!(document.contains("content: \"▸\""));
        assert!(document.contains(
            ".turn[open] > summary .arrow::before,\n.reasoning[open] > summary .arrow::before {\n    content: \"▾\";\n}"
        ));

        // The command wraps instead of scrolling sideways.
        assert!(document.contains(
            "pre.tool-block.view {\n    white-space: pre-wrap;\n    overflow-wrap: anywhere;\n}"
        ));

        // The reasoning's markdown keeps its margins inside its
        // blocks, not against the box's padding.
        assert!(document.contains(".reasoning div > :first-child"));
        assert!(document.contains(".reasoning div > :last-child"));

        // Every scrollbar is styled the same, wherever it
        // scrolls, keeping its default size.
        assert!(document.contains("scrollbar-color: var(--border) transparent"));
        assert!(document.contains("::-webkit-scrollbar-thumb"));
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
        let document = export(vec![assistant(
            "",
            "items are (Range<usize>, Code) and Box<dyn Iterator>",
            vec![],
            None,
        )]);

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
    fn a_message_follows_the_turn_that_concludes_it() {
        let document = export(vec![assistant(
            "thinking",
            "it is **done**",
            vec![],
            timings(Duration::from_millis(50)),
        )]);

        // A message alone is a turn with no work of its own:
        // its single reasoning is inlined in place of the
        // group, and the message follows it.
        assert!(!document.contains("<details class=\"turn"));
        assert!(
            document
                .contains(
                    "<details class=\"item reasoning\"><summary><span>Thought for 50ms</span><span class=\"arrow\"></span></summary><div>",
                )
        );

        let reasoning = document.find("<details class=\"item reasoning").unwrap();
        let message = document
            .find("<div class=\"item assistant\"><p>it is <strong>done</strong></p>")
            .unwrap();
        assert!(reasoning < message);

        // The reasoning carries the thinking, not the message.
        let reasoning_html = &document[reasoning..message];
        assert!(reasoning_html.contains("thinking"));
        assert!(!reasoning_html.contains("it is <strong>done</strong>"));
    }

    #[test]
    fn a_turn_groups_the_work_that_its_message_concludes() {
        let call = call(
            "call_1",
            "bash",
            r#"{"command":"git status","description":"Check the status"}"#,
        );

        let document = export(vec![
            // A message-less turn: reasoning, a tool call.
            assistant(
                "let me check the status",
                "",
                vec![call.clone()],
                timings(Duration::from_millis(50)),
            ),
            session::Item::Tool(session::ToolRun {
                call,
                status: session::Status::Success {
                    output: Output::new(),
                },
            }),
            // The message that concludes the turn.
            assistant("", "All good.", vec![], None),
        ]);

        // One turn, summarized by the command's title and the
        // reasoning's duration; its work sits behind the header.
        assert_eq!(document.matches("<details class=\"turn").count(), 1);
        assert!(
            document
                .contains("<span class=\"turn-summary\">Check the status, thought for 50ms</span>")
        );

        let turn = document.find("<details class=\"turn").unwrap();
        let message = document
            .find("<div class=\"item assistant\"><p>All good.</p>")
            .unwrap();
        assert!(turn < message);

        let turn_html = &document[turn..message];

        // The tool run and the reasoning are behind the header;
        // the message is not.
        assert!(turn_html.contains("item tool success"));
        assert!(turn_html.contains("let me check the status"));
        assert!(turn_html.contains("Thought for 50ms"));
        assert!(!turn_html.contains("All good."));
        assert!(!turn_html.contains(" open"));
    }

    #[test]
    fn consecutive_messageless_turns_are_merged_into_one_open_turn() {
        let bash1 = call("c1", "bash", r#"{"command":"make"}"#);
        let read1 = call("c2", "read", r#"{"path":"a.rs"}"#);
        let bash2 = call("c3", "bash", r#"{"command":"test"}"#);

        let document = export(vec![
            // Turn 1: reasoning + one bash call, no message.
            assistant("build it", "", vec![bash1.clone()], None),
            session::Item::Tool(session::ToolRun {
                call: bash1,
                status: session::Status::Success {
                    output: Output::new(),
                },
            }),
            // Turn 2: reasoning + a read and a bash call, no
            // message; the run ends the session, so the turn is
            // in flight and starts open.
            assistant(
                "inspect the result",
                "",
                vec![read1.clone(), bash2.clone()],
                None,
            ),
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

        // The message-less run is a single turn, open, summarizing
        // every tool across the turns.
        assert_eq!(document.matches("<details class=\"turn").count(), 1);
        assert!(document.contains("<details class=\"turn\" open>"));
        assert!(
            document.contains("<span class=\"turn-summary\">Ran 2 commands, read 1 file</span>")
        );

        // Both turns' reasoning and all three tool runs sit behind
        // the single header.
        assert!(document.contains("build it"));
        assert!(document.contains("inspect the result"));
        assert_eq!(document.matches("item tool success").count(), 3);
    }

    #[test]
    fn a_message_concludes_the_turn_that_precedes_it() {
        let bash1 = call("c1", "bash", r#"{"command":"make"}"#);
        let read1 = call("c2", "read", r#"{"path":"a.rs"}"#);

        let document = export(vec![
            // A message turn: it concludes the (empty) work that
            // precedes it.
            assistant(
                "",
                "Build it, then look at the result.",
                vec![bash1.clone()],
                None,
            ),
            // The message's own call belongs to the run that
            // follows it, which is in flight: open.
            session::Item::Tool(session::ToolRun {
                call: bash1,
                status: session::Status::Success {
                    output: Output::new(),
                },
            }),
            assistant("", "", vec![read1.clone()], None),
            session::Item::Tool(session::ToolRun {
                call: read1,
                status: session::Status::Success {
                    output: Output::new(),
                },
            }),
        ]);

        assert_eq!(document.matches("<details class=\"turn").count(), 2);

        let turn1 = document.find("<details class=\"turn").unwrap();
        let turn2 = document.rfind("<details class=\"turn").unwrap();

        // The first turn has no work of its own: a "Catching up..."
        // header, and the message below it.
        assert!(document[turn1..turn2].contains("Catching up..."));
        let message = document
            .find("<div class=\"item assistant\"><p>Build it, then look at the result.</p>")
            .unwrap();
        assert!(turn1 < message && message < turn2);

        // The second turn is in flight: open, summarizing both tool
        // runs, carrying them behind its header.
        let tail = &document[turn2..];
        assert!(tail.contains("<details class=\"turn\" open>"));
        assert!(tail.contains("<span class=\"turn-summary\">Ran 1 command, read 1 file</span>"));
        assert_eq!(tail.matches("item tool success").count(), 2);
    }

    #[test]
    fn a_lone_run_stands_in_for_its_turn() {
        // One command, no reasoning: the turn inlines its
        // single run, whose header carries the command's title.
        let document = export(vec![
            assistant(
                "",
                "",
                vec![call(
                    "c1",
                    "bash",
                    r#"{"command":"git status","description":"Check the status"}"#,
                )],
                None,
            ),
            tool(
                "c1",
                "bash",
                r#"{"command":"git status","description":"Check the status"}"#,
            ),
        ]);

        assert!(!document.contains("<details class=\"turn"));
        assert!(document.contains("<span class=\"tool-title\">Check the status</span>"));

        // Two commands: the count wins over the first title.
        let document = export(vec![
            assistant(
                "",
                "",
                vec![
                    call(
                        "c1",
                        "bash",
                        r#"{"command":"git status","description":"Check the status"}"#,
                    ),
                    call(
                        "c2",
                        "bash",
                        r#"{"command":"make","description":"Build it"}"#,
                    ),
                ],
                None,
            ),
            tool(
                "c1",
                "bash",
                r#"{"command":"git status","description":"Check the status"}"#,
            ),
            tool(
                "c2",
                "bash",
                r#"{"command":"make","description":"Build it"}"#,
            ),
        ]);

        assert!(document.contains("<span class=\"turn-summary\">Ran 2 commands</span>"));
    }

    #[test]
    fn a_run_of_unknown_tools_falls_back_to_catching_up() {
        // Unknown tools are not counted in the summary, the way
        // the app does not count them.
        let document = export(vec![
            assistant(
                "",
                "",
                vec![
                    call("c1", "grep", r#"{"pattern":"x"}"#),
                    call("c2", "grep", r#"{"pattern":"x"}"#),
                ],
                None,
            ),
            tool("c1", "grep", r#"{"pattern":"x"}"#),
            tool("c2", "grep", r#"{"pattern":"x"}"#),
        ]);

        assert!(document.contains("<span class=\"turn-summary\">Catching up...</span>"));

        // The runs still render, with their raw arguments.
        assert_eq!(document.matches("item tool success").count(), 2);
        assert!(document.contains("tool-block arguments"));
        assert!(document.contains("pattern"));
    }

    #[test]
    fn a_failed_tool_marks_its_run() {
        let document = export(vec![
            assistant(
                "",
                "",
                vec![call("call_1", "bash", r#"{"command":"false"}"#)],
                None,
            ),
            session::Item::Tool(session::ToolRun {
                call: call("call_1", "bash", r#"{"command":"false"}"#),
                status: session::Status::Error {
                    output: "boom".to_owned(),
                },
            }),
        ]);

        // The turn is in flight with a single run, so the run
        // stands in for the group; it and its output carry the
        // error status.
        assert!(!document.contains("<details class=\"turn"));
        assert!(document.contains("item tool error"));
        assert!(document.contains("<pre class=\"tool-block output error\">boom</pre>"));
    }

    #[test]
    fn a_bash_tool_shows_its_command_and_output() {
        let mut output = Output::new();
        output.push("line 1".to_owned());
        output.push("line 2".to_owned());

        let document = export(vec![session::Item::Tool(session::ToolRun {
            call: call(
                "call_1",
                "bash",
                r#"{"command":"git status","description":"Check the status"}"#,
            ),
            status: session::Status::Success { output },
        })]);

        assert!(document.contains("<details class=\"item tool success\">"));
        assert!(document.contains("<span class=\"tool-title\">Check the status</span>"));

        // The view is the command with a `$` prompt, not the raw
        // JSON.
        assert!(document.contains("tool-block view\">$ "));
        assert!(document.contains("status"));
        assert!(!document.contains("tool-block arguments"));

        // The short output renders in full, bordered by its
        // status.
        assert!(document.contains("<pre class=\"tool-block output success\">line 1\nline 2</pre>"));
    }

    #[test]
    fn an_overlong_bash_title_is_cut_at_a_word_boundary() {
        // 64 characters: over the tool's 60-character title width.
        let title = "a ".repeat(30) + "zzz";

        let document = export(vec![session::Item::Tool(session::ToolRun {
            call: call(
                "call_1",
                "bash",
                &format!(r#"{{"command":"true","description":"{title}"}}"#),
            ),
            status: session::Status::Success {
                output: Output::new(),
            },
        })]);

        // The title is cut at the last word boundary within the
        // width, with an ellipsis for what fell off.
        let cut = "a ".repeat(29) + "a…";
        assert!(document.contains(&format!("<span class=\"tool-title\">{cut}</span>")));
        assert!(!document.contains("zzz"));
    }

    #[test]
    fn an_edit_tool_shows_its_diff() {
        let document = export(vec![session::Item::Tool(session::ToolRun {
            call: call(
                "call_1",
                "edit",
                r#"{"path":"a.rs","old_string":"let x = 1;","new_string":"let x = 2;"}"#,
            ),
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
            call: call(
                "call_1",
                "write",
                r#"{"path":"a.rs","content":"let s = \"hi\";"}"#,
            ),
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
            call: call("call_1", "read", r#"{"path":"a.rs","offset":3,"limit":10}"#),
            status: session::Status::Success {
                output: Output::new(),
            },
        })]);

        // A read's title carries its path and range; it has no
        // view body.
        assert!(read.contains("<span class=\"tool-title\">a.rs (3..13)</span>"));
        let body = &read[read.find("<main").unwrap()..read.find("</main>").unwrap()];
        assert!(!body.contains("tool-block"));
        assert!(!body.contains("diff-line"));

        let mystery = export(vec![session::Item::Tool(session::ToolRun {
            call: call("call_1", "mystery", r#"{"foo":"bar"}"#),
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
    fn a_long_tool_output_keeps_every_line_and_scrolls() {
        let mut output = Output::new();

        for line in 0..20 {
            output.push(format!("line {line}"));
        }

        let document = export(vec![session::Item::Tool(session::ToolRun {
            call: call("call_1", "bash", "{}"),
            status: session::Status::Success { output },
        })]);

        // The export is the full record: every line is present.
        assert!(document.contains(">line 0\nline 1\n"));
        assert!(document.contains("line 18\nline 19</pre>"));

        // The block is capped at 10 lines and scrolls its
        // overflow, anchored at its end when the tool opens.
        assert!(document.contains("max-height: 16.5em"));
        assert!(document.contains("overflow-y: auto"));
        assert!(document.contains("block.scrollTop = block.scrollHeight"));
        assert!(!document.contains("elided"));
    }

    #[test]
    fn a_tool_error_renders_its_message() {
        let document = export(vec![session::Item::Tool(session::ToolRun {
            call: call("call_1", "bash", r#"{"command":"false"}"#),
            status: session::Status::Error {
                output: "boom & bap < 0".to_owned(),
            },
        })]);

        assert!(document.contains("<details class=\"item tool error\">"));
        assert!(
            document.contains("<pre class=\"tool-block output error\">boom &amp; bap &lt; 0</pre>")
        );
    }

    #[test]
    fn an_empty_error_output_renders_its_placeholder() {
        let document = export(vec![session::Item::Tool(session::ToolRun {
            call: call("call_1", "bash", r#"{"command":"false"}"#),
            status: session::Status::Error {
                output: "   ".to_owned(),
            },
        })]);

        assert!(document.contains("<pre class=\"tool-block output error\">[No output]</pre>"));
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

    #[test]
    fn redact_masks_the_known_secret_shapes() {
        let (text, masked) = super::redact(
            "token=ghp_abcdefghijklmnopqrstuvwxyz1234567890\n\
             Authorization: Bearer abcdefghijklmnopqrstuvwxyz\n\
             https://user:password123@example.com/v1\n\
             aws AKIAABCDEFGHIJKLMNOP\n\
             jwt eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.dozjgNryopLUGyK5",
        );

        assert!(!text.contains("ghp_"));
        assert!(!text.contains("Bearer abcdef"));
        assert!(text.contains("https://user:•••@example.com"));
        assert!(!text.contains("AKIA"));
        assert!(!text.contains("eyJhbG"));
        assert!(masked >= 5);
    }

    #[test]
    fn redact_masks_a_secret_assignment_and_not_a_common_word() {
        let (text, masked) = super::redact("API_KEY=abc123\ndevice: monkey\nmonkey: 3\n");

        assert!(text.starts_with("API_KEY=•••"));
        assert!(text.contains("device: monkey"));
        assert!(text.contains("monkey: 3"));
        assert_eq!(masked, 1);
    }

    #[test]
    fn redact_abbreviates_the_home_directory() {
        let (text, count) =
            super::abbreviate_home_in("in /home/hector/projects and /home/hector", "/home/hector");

        assert_eq!(text, "in ~/projects and ~");
        assert_eq!(count, 2);
    }

    #[test]
    fn a_secret_in_a_tool_output_is_masked() {
        let mut output = Output::new();
        output.push("API_KEY=sk-abcdefghijklmnopqrstuvwxyz1234567890".to_owned());

        let run = session::Item::Tool(session::ToolRun {
            call: call("c1", "bash", r#"{"command":"echo $API_KEY"}"#),
            status: session::Status::Success { output },
        });

        let (document, redactions) = export_all(vec![run], None, false);

        assert!(document.contains("•••"));
        assert!(!document.contains("sk-abcdef"));
        assert!(redactions.masked > 0);
    }

    #[test]
    fn an_env_listing_is_concealed() {
        let mut output = Output::new();
        output.push("API_KEY=abc123".to_owned());

        let run = session::Item::Tool(session::ToolRun {
            call: call("c1", "bash", r#"{"command":"env"}"#),
            status: session::Status::Success { output },
        });

        let (document, redactions) = export_all(vec![run], None, false);

        assert!(document.contains("[redacted: may contain credentials]"));
        assert!(!document.contains("abc123"));
        assert_eq!(redactions.blocks, 1);
    }

    #[test]
    fn a_read_of_a_credentials_file_is_concealed() {
        let mut output = Output::new();
        output.push("machine api.example login: bot password: hunter2".to_owned());

        let run = session::Item::Tool(session::ToolRun {
            call: call("c1", "read", r#"{"path":".netrc"}"#),
            status: session::Status::Success { output },
        });

        let (document, redactions) = export_all(vec![run], None, false);

        assert!(document.contains("[redacted: may contain credentials]"));
        assert!(!document.contains("hunter2"));
        assert_eq!(redactions.blocks, 1);
    }

    #[test]
    fn a_raw_export_conceals_nothing() {
        let mut output = Output::new();
        output.push("API_KEY=abc123".to_owned());

        let run = session::Item::Tool(session::ToolRun {
            call: call("c1", "bash", r#"{"command":"env"}"#),
            status: session::Status::Success { output },
        });

        let (document, redactions) = export_all(vec![run], None, true);

        assert!(document.contains("API_KEY=abc123"));
        assert!(redactions.is_empty());
    }
}
