use crate::Output;
use crate::file;

use std::path::Path;
use std::time::SystemTime;

#[derive(Debug, Clone)]
pub struct Session {
    pub version: Version,
    pub started_at: SystemTime,
    pub items: Vec<Item>,
}

impl Session {
    /// Loads the session at `path`, or an empty session when the
    /// file does not exist yet: the first `append` creates it.
    pub async fn load(path: impl AsRef<Path>) -> Result<Self, reason::Error> {
        use std::io::BufRead;

        let mut session = Self {
            version: Version::current(),
            started_at: SystemTime::now(),
            items: Vec::new(),
        };

        let path = path.as_ref().to_path_buf();

        tokio::task::spawn_blocking(move || {
            let file = match std::fs::File::open(&path) {
                Ok(file) => file,
                // A missing file is no error: the session does not
                // exist yet.
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(session),
                Err(error) => return Err(error.into()),
            };

            let mut reader = std::io::BufReader::new(file);
            let mut line = String::new();

            while reader.read_line(&mut line)? > 0 {
                if line.trim().is_empty() {
                    continue;
                }

                let frame = decoder::run(serde_json::from_str, Frame::decode, &line)
                    .map_err(std::io::Error::from)?;

                match frame.event {
                    Event::Created(version) => {
                        session.version = version;
                        session.started_at = frame.at;
                    }
                    Event::ItemAdded(item) => {
                        session.items.push(item);
                    }
                }

                line.clear();
            }

            session.abort_interrupted_tools();

            Ok(session)
        })
        .await?
    }

    pub async fn append(
        path: impl AsRef<Path>,
        events: impl IntoIterator<Item = Event> + Send + 'static,
    ) -> Result<SystemTime, reason::Error> {
        use tokio::io::AsyncWriteExt;

        // Appends to the same session are serialized, so the
        // `created` frame is written by exactly one of them.
        let _lock = file::lock(&path).await;

        let now = SystemTime::now();

        if !tokio::fs::try_exists(&path).await? {
            let mut created = serde_json::to_string(
                &Frame {
                    event: Event::Created(Version::current()),
                    at: now,
                }
                .encode(),
            )?;
            created.push('\n');

            tokio::fs::write(&path, created).await?;
        }

        let json = tokio::task::spawn_blocking(move || {
            events
                .into_iter()
                .map(|event| serde_json::to_string(&Frame { event, at: now }.encode()))
                .collect::<Result<Vec<_>, _>>()
        })
        .await??
        .join("\n");

        if json.trim().is_empty() {
            return Ok(now);
        }

        let file = tokio::fs::OpenOptions::new()
            .append(true)
            .open(path)
            .await?;

        let mut writer = tokio::io::BufWriter::new(file);
        writer.write_all(&json.into_bytes()).await?;
        writer.write_all(b"\n").await?;
        writer.flush().await?;

        Ok(now)
    }

    fn abort_interrupted_tools(&mut self) {
        let Some((i, Item::Assistant(reply))) = self
            .items
            .iter()
            .enumerate()
            .rfind(|(_, item)| matches!(item, Item::Assistant(_)))
        else {
            return;
        };

        let missing = reply
            .tool_calls
            .iter()
            .filter(|call| {
                !self.items[i + 1..]
                    .iter()
                    .any(|item| matches!(item, Item::Tool(tool) if tool.call.id == call.id))
            })
            .cloned()
            .collect::<Vec<_>>();

        for call in missing {
            self.items.push(Item::Tool(ToolRun {
                call,
                status: Status::Aborted,
            }));
        }
    }
}

#[derive(Debug, Clone)]
pub struct Frame {
    pub event: Event,
    pub at: SystemTime,
}

impl Frame {
    fn encode(&self) -> decoder::Value {
        use decoder::encode::{duration, map};

        let posix = self
            .at
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default();

        self.event
            .encode()
            .extend(map([("at", duration(posix))]))
            .into_value()
    }

    fn decode(value: decoder::Value) -> decoder::Result<Self> {
        use decoder::decode::{duration, map};

        let mut fields = map(value)?;

        let at = SystemTime::UNIX_EPOCH + fields.required("at", duration)?;

        Ok(Self {
            event: Event::decode(fields.into_value())?,
            at,
        })
    }
}

#[derive(Debug, Clone)]
pub enum Event {
    Created(Version),
    ItemAdded(Item),
}

impl Event {
    fn encode(&self) -> decoder::Map {
        use decoder::encode::map;

        let (type_, event) = match self {
            Event::Created(version) => ("created", map([("version", version.encode())])),
            Event::ItemAdded(item) => ("item_added", item.encode()),
        };

        event.tag("event", type_)
    }

    fn decode(value: decoder::Value) -> decoder::Result<Self> {
        use decoder::decode::{map, string};

        let mut fields = map(value)?;

        match fields.required("event", string)?.as_str() {
            "created" => Ok(Self::Created(fields.required("version", Version::decode)?)),
            "item_added" => Ok(Self::ItemAdded(Item::decode(fields.into_value())?)),
            other => Err(decoder::Error::custom(format!("invalid event: {other}"))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(String);

impl Version {
    fn current() -> Self {
        Self(env!("CARGO_PKG_VERSION").to_owned())
    }

    fn encode(&self) -> decoder::Value {
        use decoder::encode::string;

        string(&self.0)
    }

    fn decode(value: decoder::Value) -> decoder::Result<Self> {
        use decoder::decode::string;

        string(value).map(Self)
    }
}

#[derive(Debug, Clone)]
pub enum Item {
    User(String),
    Assistant(Reply),
    Tool(ToolRun),
    Compaction(Compaction),
}

impl Item {
    fn encode(&self) -> decoder::Map {
        use decoder::encode::{map, string};

        let (type_, item) = match self {
            Item::User(content) => ("user", map([("content", string(content))])),
            Item::Assistant(reply) => ("assistant", reply.encode()),
            Item::Tool(tool_run) => ("tool", tool_run.encode()),
            Item::Compaction(compaction) => ("compaction", compaction.encode()),
        };

        item.tag("item", type_)
    }

    fn decode(value: decoder::Value) -> decoder::Result<Self> {
        use decoder::decode::{map, string};

        let mut fields = map(value)?;

        Ok(match fields.required("item", string)?.as_str() {
            "user" => Self::User(fields.required("content", string)?),
            "assistant" => Self::Assistant(Reply::decode(fields.into_value())?),
            "tool" => Self::Tool(ToolRun::decode(fields.into_value())?),
            "compaction" => Self::Compaction(Compaction::decode(fields.into_value())?),
            other => return Err(decoder::Error::custom(format!("invalid item: {other}"))),
        })
    }
}

#[derive(Debug, Clone, Default)]
pub struct Reply {
    pub prompt: reason::Progress,
    pub reasoning: String,
    pub content: String,
    pub tool_calls: Vec<reason::tool::Call>,
    pub timings: Option<reason::Timings>,
}

impl Reply {
    fn encode(&self) -> decoder::Map {
        use decoder::encode::{map, optional, sequence, string};

        map([
            ("prompt", progress::encode(self.prompt)),
            ("reasoning", string(&self.reasoning)),
            ("content", string(&self.content)),
            ("tool_calls", sequence(tool_call::encode, &self.tool_calls)),
            ("timings", optional(timings::encode, self.timings)),
        ])
    }

    fn decode(value: decoder::Value) -> decoder::Result<Self> {
        use decoder::decode::{map, optional, sequence, string};

        let mut fields = map(value)?;

        Ok(Self {
            prompt: fields.required("prompt", progress::decode)?,
            reasoning: fields.required("reasoning", string)?,
            content: fields.required("content", string)?,
            tool_calls: fields.required("tool_calls", sequence(tool_call::decode))?,
            timings: fields.required("timings", optional(timings::decode))?,
        })
    }
}

mod progress {
    pub fn encode(progress: reason::Progress) -> decoder::Value {
        use decoder::encode::{map, u64};

        map([
            ("total", u64(progress.total)),
            ("processed", u64(progress.processed)),
            ("cached", u64(progress.cached)),
        ])
        .into_value()
    }

    pub fn decode(value: decoder::Value) -> decoder::Result<reason::Progress> {
        use decoder::decode::{map, u64};

        let mut fields = map(value)?;

        Ok(reason::Progress {
            total: fields.required("total", u64)?,
            processed: fields.required("processed", u64)?,
            cached: fields.required("cached", u64)?,
        })
    }
}

mod tool_call {
    pub fn encode(call: &reason::tool::Call) -> decoder::Value {
        use decoder::encode::{map, string};

        map([
            ("id", string(call.id.as_str())),
            ("name", string(&call.name)),
            ("arguments", string(&call.arguments)),
        ])
        .into_value()
    }

    pub fn decode(value: decoder::Value) -> decoder::Result<reason::tool::Call> {
        use decoder::decode::{map, string};

        let mut fields = map(value)?;

        Ok(reason::tool::Call {
            id: fields.required("id", string)?.into(),
            name: fields.required("name", string)?,
            arguments: fields.required("arguments", string)?,
        })
    }
}

mod timings {
    pub fn encode(timings: reason::Timings) -> decoder::Value {
        use decoder::encode::{duration, map, u64};

        fn generation(generation: reason::Generation) -> decoder::Value {
            map([
                ("amount", u64(generation.amount)),
                ("total", duration(generation.total)),
                ("token", duration(generation.token)),
            ])
            .into_value()
        }

        map([
            ("cached", u64(timings.cached)),
            ("prompt", generation(timings.prompt)),
            ("predicted", generation(timings.predicted)),
            ("reasoning", duration(timings.reasoning)),
        ])
        .into_value()
    }

    pub fn decode(value: decoder::Value) -> decoder::Result<reason::Timings> {
        use decoder::decode::{duration, map, u64};

        fn generation(value: decoder::Value) -> decoder::Result<reason::Generation> {
            let mut fields = map(value)?;

            Ok(reason::Generation {
                amount: fields.required("amount", u64)?,
                total: fields.required("total", duration)?,
                token: fields.required("token", duration)?,
            })
        }

        let mut fields = map(value)?;

        Ok(reason::Timings {
            cached: fields.required("cached", u64)?,
            prompt: fields.required("prompt", generation)?,
            predicted: fields.required("predicted", generation)?,
            reasoning: fields.required("reasoning", duration)?,
        })
    }
}

#[derive(Debug, Clone)]
pub struct ToolRun {
    pub call: reason::tool::Call,
    pub status: Status,
}

impl ToolRun {
    fn encode(&self) -> decoder::Map {
        use decoder::encode::map;

        map([("call", tool_call::encode(&self.call))]).extend(self.status.encode())
    }

    fn decode(value: decoder::Value) -> decoder::Result<Self> {
        use decoder::decode::map;

        let mut fields = map(value)?;

        let call = fields.required("call", tool_call::decode)?;
        let status = Status::decode(fields.into_value())?;

        Ok(Self { call, status })
    }
}

#[derive(Debug, Clone)]
pub enum Status {
    Success { output: Output },
    Error { output: String },
    Invalid,
    Aborted,
}

impl Status {
    fn encode(&self) -> decoder::Map {
        use decoder::encode::{map, string};

        let (type_, status) = match self {
            Status::Success { output } => ("success", map([("output", output.encode())])),
            Status::Error { output } => ("error", map([("error", string(output))])),
            Status::Invalid => ("invalid", map([])),
            Status::Aborted => ("aborted", map([])),
        };

        status.tag("status", type_)
    }

    fn decode(value: decoder::Value) -> decoder::Result<Self> {
        use decoder::decode::{map, string};

        let mut fields = map(value)?;

        match fields.required("status", string)?.as_str() {
            "success" => Ok(Self::Success {
                output: fields.required("output", Output::decode)?,
            }),
            "error" => Ok(Self::Error {
                output: fields.required("error", string)?,
            }),
            "invalid" => Ok(Self::Invalid),
            "aborted" => Ok(Self::Aborted),
            other => Err(decoder::Error::custom(format!("invalid status: {other}"))),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Compaction {
    pub reply: Reply,
    pub tokens: u64,
    pub reasoning_tokens: u64,
    pub to: usize,
}

impl Compaction {
    fn encode(&self) -> decoder::Map {
        use decoder::encode::{map, u64};

        self.reply.encode().extend(map([
            ("tokens", u64(self.tokens)),
            ("reasoning_tokens", u64(self.reasoning_tokens)),
            ("to", u64(self.to as u64)),
        ]))
    }

    fn decode(value: decoder::Value) -> decoder::Result<Self> {
        use decoder::decode::{map, u64};

        let mut fields = map(value)?;

        let tokens = fields.required("tokens", u64)?;
        let reasoning_tokens = fields.required("reasoning_tokens", u64)?;
        let to = fields.required("to", u64)? as usize;
        let reply = Reply::decode(fields.into_value())?;

        Ok(Self {
            reply,
            tokens,
            reasoning_tokens,
            to,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn at(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
    }

    fn call() -> reason::tool::Call {
        reason::tool::Call {
            id: "call_1".to_owned().into(),
            name: "read".to_owned(),
            arguments: r#"{"path": "Cargo.toml"}"#.to_owned(),
        }
    }

    fn call_a() -> reason::tool::Call {
        reason::tool::Call {
            id: "call_a".to_owned().into(),
            name: "bash".to_owned(),
            arguments: r#"{"command": "cargo build"}"#.to_owned(),
        }
    }

    fn call_b() -> reason::tool::Call {
        reason::tool::Call {
            id: "call_b".to_owned().into(),
            name: "read".to_owned(),
            arguments: r#"{"path": "src/main.rs"}"#.to_owned(),
        }
    }

    fn assistant(calls: Vec<reason::tool::Call>) -> Item {
        Item::Assistant(Reply {
            prompt: Default::default(),
            reasoning: String::new(),
            content: "running the build".to_owned(),
            tool_calls: calls,
            timings: None,
        })
    }

    fn roundtrip(frame: Frame) -> Frame {
        Frame::decode(frame.encode()).expect("frame roundtrips")
    }

    #[test]
    fn a_created_frame_roundtrips() {
        let decoded = roundtrip(Frame {
            event: Event::Created(Version::current()),
            at: at(1_736_000_000),
        });

        assert_eq!(decoded.at, at(1_736_000_000));
        assert!(matches!(decoded.event, Event::Created(_)));
    }

    #[test]
    fn a_user_item_roundtrips() {
        let decoded = roundtrip(Frame {
            event: Event::ItemAdded(Item::User("hello".to_owned())),
            at: at(1_736_000_001),
        });

        assert_eq!(decoded.at, at(1_736_000_001));
        assert!(matches!(
            decoded.event,
            Event::ItemAdded(Item::User(content)) if content == "hello"
        ));
    }

    #[test]
    fn an_assistant_item_roundtrips() {
        let frame = Frame {
            event: Event::ItemAdded(Item::Assistant(Reply {
                prompt: reason::Progress {
                    total: 100,
                    processed: 10,
                    cached: 5,
                },
                reasoning: "thinking...".to_owned(),
                content: "the answer".to_owned(),
                tool_calls: vec![call()],
                timings: Some(reason::Timings {
                    cached: 3,
                    prompt: reason::Generation {
                        amount: 10,
                        total: Duration::from_millis(100),
                        token: Duration::from_millis(10),
                    },
                    predicted: reason::Generation {
                        amount: 20,
                        total: Duration::from_millis(200),
                        token: Duration::from_millis(20),
                    },
                    reasoning: Duration::from_millis(50),
                }),
            })),
            at: at(1_736_000_002),
        };

        let decoded = roundtrip(frame);

        assert_eq!(decoded.at, at(1_736_000_002));

        let Event::ItemAdded(Item::Assistant(reply)) = decoded.event else {
            panic!("expected an assistant item");
        };

        assert_eq!(
            reply.prompt,
            reason::Progress {
                total: 100,
                processed: 10,
                cached: 5
            }
        );
        assert_eq!(reply.reasoning, "thinking...");
        assert_eq!(reply.content, "the answer");
        assert_eq!(reply.tool_calls.len(), 1);
        assert_eq!(reply.tool_calls[0].id.as_str(), "call_1");
        assert_eq!(reply.tool_calls[0].name, "read");
        assert_eq!(reply.tool_calls[0].arguments, r#"{"path": "Cargo.toml"}"#);

        let timings = reply.timings.expect("timings are present");
        assert_eq!(timings.cached, 3);
        assert_eq!(timings.prompt.amount, 10);
        assert_eq!(timings.predicted.amount, 20);
        assert_eq!(timings.reasoning, Duration::from_millis(50));
    }

    #[test]
    fn an_assistant_item_without_timings_roundtrips() {
        let decoded = roundtrip(Frame {
            event: Event::ItemAdded(Item::Assistant(Reply {
                prompt: Default::default(),
                reasoning: String::new(),
                content: "just content".to_owned(),
                tool_calls: vec![],
                timings: None,
            })),
            at: at(1_736_000_003),
        });

        let Event::ItemAdded(Item::Assistant(reply)) = decoded.event else {
            panic!("expected an assistant item");
        };

        assert!(reply.timings.is_none());
        assert!(reply.tool_calls.is_empty());
    }

    #[test]
    fn tool_items_roundtrip_each_status() {
        let mut output = Output::new();
        output.push("line 1".to_owned());
        output.push("line 2".to_owned());

        for status in [
            Status::Success {
                output: output.clone(),
            },
            Status::Error {
                output: "boom".to_owned(),
            },
            Status::Invalid,
            Status::Aborted,
        ] {
            let decoded = roundtrip(Frame {
                event: Event::ItemAdded(Item::Tool(ToolRun {
                    call: call(),
                    status,
                })),
                at: at(1_736_000_004),
            });

            assert_eq!(decoded.at, at(1_736_000_004));

            let Event::ItemAdded(Item::Tool(run)) = decoded.event else {
                panic!("expected a tool item");
            };

            assert_eq!(run.call.id.as_str(), "call_1");
            assert_eq!(run.call.name, "read");
            assert_eq!(run.call.arguments, r#"{"path": "Cargo.toml"}"#);

            match run.status {
                Status::Success { output } => assert_eq!(output.to_string(), "line 1\nline 2"),
                Status::Error { output } => assert_eq!(output, "boom"),
                Status::Invalid => {}
                Status::Aborted => {}
            }
        }
    }

    #[test]
    fn a_compaction_item_roundtrips() {
        let decoded = roundtrip(Frame {
            event: Event::ItemAdded(Item::Compaction(Compaction {
                reply: Reply {
                    prompt: Default::default(),
                    reasoning: "condensing".to_owned(),
                    content: "summary".to_owned(),
                    tool_calls: vec![],
                    timings: None,
                },
                tokens: 1_000,
                reasoning_tokens: 100,
                to: 42,
            })),
            at: at(1_736_000_005),
        });

        assert_eq!(decoded.at, at(1_736_000_005));

        let Event::ItemAdded(Item::Compaction(compaction)) = decoded.event else {
            panic!("expected a compaction item");
        };

        assert_eq!(compaction.tokens, 1_000);
        assert_eq!(compaction.reasoning_tokens, 100);
        assert_eq!(compaction.to, 42);
        assert_eq!(compaction.reply.content, "summary");
    }

    #[test]
    fn an_invalid_event_is_rejected() {
        let frame = Frame {
            event: Event::Created(Version::current()),
            at: at(1_736_000_006),
        };

        let json = serde_json::to_string(&frame.encode()).expect("encode frame");
        let json = json.replacen("\"created\"", "\"exploded\"", 1);

        let decoded: decoder::Result<Frame> =
            decoder::run(serde_json::from_str, Frame::decode, &json);

        assert!(decoded.is_err());
    }

    #[tokio::test]
    async fn a_session_loads_what_it_appends() {
        let path = std::env::temp_dir().join(format!("pick-session-test-{}", std::process::id()));
        std::fs::remove_file(&path).ok();

        let at = Session::append(&path, [Event::ItemAdded(Item::User("hello".to_owned()))])
            .await
            .expect("append item frame");

        let session = Session::load(&path).await.expect("load session");
        std::fs::remove_file(&path).ok();

        assert_eq!(session.started_at, at);
        assert!(matches!(
            session.items.as_slice(),
            [Item::User(content)] if content == "hello"
        ));
    }

    #[tokio::test]
    async fn a_missing_file_loads_an_empty_session() {
        let path =
            std::env::temp_dir().join(format!("pick-session-missing-{}", std::process::id()));
        std::fs::remove_file(&path).ok();

        let session = Session::load(&path).await.expect("load missing session");

        assert!(session.items.is_empty());
    }

    #[tokio::test]
    async fn concurrent_appends_to_the_same_session_are_serialized() {
        let path = std::env::temp_dir().join(format!("pick-session-race-{}", std::process::id()));
        std::fs::remove_file(&path).ok();

        let mut tasks = Vec::new();

        for n in 0..8 {
            let path = path.clone();

            tasks.push(tokio::spawn(async move {
                Session::append(&path, [Event::ItemAdded(Item::User(format!("item {n}")))])
                    .await
                    .expect("append item frame")
            }));
        }

        for task in tasks {
            task.await.expect("append task");
        }

        let contents = std::fs::read_to_string(&path).expect("read session file");

        // No two appends may both create the file, so there is
        // exactly one `created` frame, and no item frame is lost:
        // one plus eight lines.
        assert_eq!(contents.lines().count(), 9);

        let session = Session::load(&path).await.expect("load session");
        std::fs::remove_file(&path).ok();

        assert_eq!(session.items.len(), 8);
    }

    #[test]
    fn interrupted_tools_are_answered_with_aborted() {
        let mut session = Session {
            version: Version::current(),
            started_at: at(1_736_000_010),
            items: vec![
                Item::User("build it".to_owned()),
                assistant(vec![call_a(), call_b()]),
            ],
        };

        session.abort_interrupted_tools();

        let [
            Item::User(_),
            Item::Assistant(_),
            Item::Tool(first),
            Item::Tool(second),
        ] = session.items.as_slice()
        else {
            panic!("expected the calls to be answered");
        };

        // In call order, with the calls' fields intact.
        assert_eq!(first.call.id.as_str(), "call_a");
        assert_eq!(first.call.name, "bash");
        assert_eq!(first.call.arguments, r#"{"command": "cargo build"}"#);
        assert!(matches!(first.status, Status::Aborted));

        assert_eq!(second.call.id.as_str(), "call_b");
        assert!(matches!(second.status, Status::Aborted));
    }

    #[test]
    fn answered_calls_are_left_alone() {
        let mut session = Session {
            version: Version::current(),
            started_at: at(1_736_000_011),
            items: vec![
                Item::User("build it".to_owned()),
                assistant(vec![call_a()]),
                Item::Tool(ToolRun {
                    call: call_a(),
                    status: Status::Success {
                        output: Output::new(),
                    },
                }),
            ],
        };

        session.abort_interrupted_tools();

        assert_eq!(session.items.len(), 3);
    }

    #[test]
    fn answering_does_not_shift_a_compaction() {
        let mut session = Session {
            version: Version::current(),
            started_at: at(1_736_000_012),
            items: vec![
                Item::User("earlier".to_owned()),
                assistant(vec![]),
                Item::Compaction(Compaction {
                    reply: Reply {
                        prompt: Default::default(),
                        reasoning: String::new(),
                        content: "summary".to_owned(),
                        tool_calls: vec![],
                        timings: None,
                    },
                    tokens: 1_000,
                    reasoning_tokens: 0,
                    to: 1,
                }),
                Item::User("build it".to_owned()),
                assistant(vec![call_a()]),
            ],
        };

        session.abort_interrupted_tools();

        // The answer is appended after the compaction, so its
        // cutoff — and the slice it delimits — is untouched.
        let Item::Compaction(compaction) = &session.items[2] else {
            panic!("expected the compaction at index 2");
        };

        assert_eq!(compaction.to, 1);
        assert_eq!(session.items.len(), 6);
        assert!(matches!(
            session.items.last(),
            Some(Item::Tool(tool)) if matches!(tool.status, Status::Aborted)
        ));
    }

    #[tokio::test]
    async fn a_load_answers_the_calls_a_crash_left_open() {
        let path = std::env::temp_dir().join(format!("pick-session-crash-{}", std::process::id()));
        std::fs::remove_file(&path).ok();

        // A batch of two, the first settled, the second still
        // running when the process died: the settle filter saved the
        // assistant and the first tool, and stopped there.
        Session::append(
            &path,
            [
                Event::ItemAdded(Item::User("build it".to_owned())),
                Event::ItemAdded(assistant(vec![call_a(), call_b()])),
                Event::ItemAdded(Item::Tool(ToolRun {
                    call: call_a(),
                    status: Status::Success {
                        output: Output::new(),
                    },
                })),
            ],
        )
        .await
        .expect("append frames");

        let session = Session::load(&path).await.expect("load session");
        std::fs::remove_file(&path).ok();

        // The settled tool is untouched; the one the crash left
        // running is answered, after it.
        let [
            Item::User(_),
            Item::Assistant(_),
            Item::Tool(settled),
            Item::Tool(answered),
        ] = session.items.as_slice()
        else {
            panic!("expected the crash's call to be answered");
        };

        assert!(matches!(settled.status, Status::Success { .. }));
        assert_eq!(settled.call.id.as_str(), "call_a");
        assert_eq!(answered.call.id.as_str(), "call_b");
        assert!(matches!(answered.status, Status::Aborted));
    }
}
