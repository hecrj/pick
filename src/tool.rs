use serde::Deserialize;
use serde::de::DeserializeOwned;
use tokio::io::AsyncReadExt;

use std::collections::HashMap;
use std::path::Path;
use std::pin::Pin;

type Result = ::core::result::Result<String, reason::Error>;
type Future = Pin<Box<dyn std::future::Future<Output = Result> + Send>>;

pub struct Tool {
    name: &'static str,
    description: &'static str,
    parameters: &'static [Parameter],
    run: Box<dyn Fn(&Path, &str) -> Future>,
}

impl Tool {
    pub fn builtins() -> HashMap<&'static str, Self> {
        const READ_CHUNK_SIZE: usize = 64 * 1024;
        const DEFAULT_READ_LIMIT: u64 = 1_000;
        const MAX_READ_LIMIT: u64 = 10_000;
        const MAX_READ_BYTES: u64 = 50 * 1024;

        #[derive(Deserialize)]
        struct Bash {
            command: String,
        }

        fn bash(project: &Path, arguments: Bash) -> Future {
            let project = project.to_path_buf();

            Box::pin(async move {
                let command = format!("exec 2>&1; {}", arguments.command);

                let output = tokio::process::Command::new("bash")
                    .args(["-c", &command])
                    .current_dir(project)
                    .output()
                    .await?;

                let stdout = String::from_utf8_lossy(&output.stdout).into_owned();

                if output.status.success() {
                    Ok(stdout)
                } else {
                    Err(std::io::Error::other(format!(
                        "{status}\n{stdout}",
                        status = output.status
                    )))?
                }
            })
        }

        #[derive(Deserialize)]
        struct Read {
            path: String,
            #[serde(default)]
            offset: Option<u64>,
            #[serde(default)]
            limit: Option<u64>,
        }

        fn read(project: &Path, arguments: Read) -> Future {
            let project = project.to_path_buf();

            Box::pin(async move {
                let offset = match arguments.offset {
                    Some(offset) if offset > 0 => offset,
                    Some(_) => Err(std::io::Error::other(
                        "offset must be a 1-based line number (>= 1)",
                    ))?,
                    None => 1,
                };

                let limit = match arguments.limit {
                    Some(limit) if limit > 0 => limit.min(MAX_READ_LIMIT),
                    Some(_) => Err(std::io::Error::other("limit must be >= 1"))?,
                    None => DEFAULT_READ_LIMIT,
                };

                let path = project.join(&arguments.path);
                let mut file = tokio::fs::File::open(&path).await?;

                #[derive(Debug, PartialEq)]
                enum Stop {
                    EndOfFile,
                    LineLimit { has_more: bool },
                    ByteLimit,
                }

                let mut stop = Stop::EndOfFile;
                let mut chunk = vec![0u8; READ_CHUNK_SIZE];
                let mut scanned = 0u64;
                let mut terminated_lines = 0u64;
                let mut emitted = 0u64;
                let mut pending = Vec::new();
                let mut output = String::new();

                loop {
                    let n = file.read(&mut chunk).await?;

                    if n == 0 {
                        break;
                    }

                    scanned += n as u64;

                    for &byte in &chunk[..n] {
                        if byte == b'\n' {
                            terminated_lines += 1;

                            if terminated_lines >= offset {
                                if pending.last() == Some(&b'\r') {
                                    pending.pop();
                                }

                                output.push_str(&String::from_utf8_lossy(&pending));
                                output.push('\n');
                                emitted += 1;
                            }

                            pending.clear();
                        } else {
                            pending.push(byte);
                        }

                        if emitted == limit {
                            break;
                        }
                    }

                    if emitted == limit {
                        // Peek one more chunk to see whether the file continues.
                        stop = Stop::LineLimit {
                            has_more: file.read(&mut chunk).await? > 0,
                        };

                        break;
                    }

                    if scanned >= MAX_READ_BYTES {
                        stop = Stop::ByteLimit;

                        break;
                    }
                }

                // A final line may not end with a newline.
                if matches!(stop, Stop::EndOfFile) && !pending.is_empty() {
                    if pending.last() == Some(&b'\r') {
                        pending.pop();
                    }

                    if terminated_lines + 1 >= offset {
                        output.push_str(&String::from_utf8_lossy(&pending));
                        output.push('\n');
                        emitted += 1;
                    }

                    terminated_lines += 1;
                }

                if output.is_empty() && matches!(stop, Stop::EndOfFile) {
                    return Ok(if terminated_lines == 0 {
                        "File is empty.".to_owned()
                    } else {
                        format!(
                            "File has {} line{}; offset {} is past the end.",
                            terminated_lines,
                            if terminated_lines == 1 { "" } else { "s" },
                            offset
                        )
                    });
                }

                if output.is_empty() {
                    return Ok(format!(
                        "No complete lines could be read within the {MAX_READ_BYTES} byte budget; the file's lines may be very long."
                    ));
                }

                match stop {
                    Stop::EndOfFile => Ok(output),
                    Stop::LineLimit { has_more } => {
                        if has_more {
                            Ok(format!(
                                "{output}\n[File has more lines; continue reading with offset={}]",
                                offset + emitted
                            ))
                        } else {
                            Ok(output)
                        }
                    }
                    Stop::ByteLimit => Ok(format!(
                        "{output}\n[Read stopped after scanning {MAX_READ_BYTES} bytes; some lines may be very long. Try a smaller limit or use bash to inspect the file.]"
                    )),
                }
            })
        }

        #[derive(Deserialize)]
        struct Write {
            path: String,
            content: String,
        }

        fn write(project: &Path, arguments: Write) -> Future {
            let project = project.to_path_buf();

            Box::pin(async move {
                let path = project.join(&arguments.path);

                tokio::fs::write(&path, arguments.content).await?;

                Ok(format!("Wrote to {}", path.display()))
            })
        }

        #[derive(Deserialize)]
        struct Edit {
            path: String,
            old_string: String,
            new_string: String,
            #[serde(default)]
            replace_all: bool,
        }

        fn edit(project: &Path, arguments: Edit) -> Future {
            let project = project.to_path_buf();

            Box::pin(async move {
                let path = project.join(&arguments.path);
                let contents = tokio::fs::read_to_string(&path).await?;

                if arguments.old_string.is_empty() {
                    Err(std::io::Error::other("old_string must not be empty"))?
                }

                if arguments.old_string == arguments.new_string {
                    Err(std::io::Error::other(
                        "old_string and new_string must be different",
                    ))?
                }

                let occurrences = contents.matches(&arguments.old_string).count();

                if occurrences == 0 {
                    Err(std::io::Error::other(format!(
                        "old_string not found in {}",
                        path.display()
                    )))?
                }

                if occurrences > 1 && !arguments.replace_all {
                    Err(std::io::Error::other(format!(
                        "old_string matches {occurrences} locations in {}; include more context to make it unique, or set replace_all to true",
                        path.display()
                    )))?
                }

                let updated = if arguments.replace_all {
                    contents.replace(&arguments.old_string, &arguments.new_string)
                } else {
                    contents.replacen(&arguments.old_string, &arguments.new_string, 1)
                };

                tokio::fs::write(&path, updated).await?;

                if arguments.replace_all {
                    Ok(format!(
                        "Edited {} ({} replacements)",
                        path.display(),
                        occurrences
                    ))
                } else {
                    Ok(format!("Edited {} (1 replacement)", path.display()))
                }
            })
        }

        let tools = [
            Self::new(
                "read",
                "Read file contents",
                &[
                    Parameter {
                        name: "path",
                        description: "Path of the file",
                        schema: Schema::String,
                        required: true,
                    },
                    Parameter {
                        name: "offset",
                        description: "1-based line number to start reading from",
                        schema: Schema::Integer,
                        required: false,
                    },
                    Parameter {
                        name: "limit",
                        description: "Maximum number of lines to read",
                        schema: Schema::Integer,
                        required: false,
                    },
                ],
                read,
            ),
            Self::new(
                "bash",
                "Run a bash command",
                &[Parameter {
                    name: "command",
                    description: "Command to run",
                    schema: Schema::String,
                    required: true,
                }],
                bash,
            ),
            Self::new(
                "write",
                "Write file contents",
                &[
                    Parameter {
                        name: "path",
                        description: "Path of the file",
                        schema: Schema::String,
                        required: true,
                    },
                    Parameter {
                        name: "content",
                        description: "Contents to write to the file",
                        schema: Schema::String,
                        required: true,
                    },
                ],
                write,
            ),
            Self::new(
                "edit",
                "Edit a file by replacing an exact string match. \
                old_string must match exactly one location in the file, \
                including whitespace and indentation, unless replace_all is set",
                &[
                    Parameter {
                        name: "path",
                        description: "Path of the file",
                        schema: Schema::String,
                        required: true,
                    },
                    Parameter {
                        name: "old_string",
                        description: "Exact text to replace",
                        schema: Schema::String,
                        required: true,
                    },
                    Parameter {
                        name: "new_string",
                        description: "Text to replace it with",
                        schema: Schema::String,
                        required: true,
                    },
                    Parameter {
                        name: "replace_all",
                        description: "Replace all occurrences instead of requiring a unique match",
                        schema: Schema::Boolean,
                        required: false,
                    },
                ],
                edit,
            ),
        ];

        HashMap::from_iter(tools.into_iter().map(|tool| (tool.name, tool)))
    }

    pub fn run(&self, project: impl AsRef<Path>, arguments: &str) -> Future {
        (self.run)(project.as_ref(), arguments)
    }

    pub fn to_metadata(&self) -> reason::Tool {
        reason::Tool::Function {
            function: reason::tool::Function {
                name: self.name.to_owned(),
                description: self.description.to_owned(),
                parameters: reason::tool::Schema::Object {
                    description: None,
                    properties: self
                        .parameters
                        .iter()
                        .map(|param| {
                            (
                                param.name.to_owned(),
                                match param.schema {
                                    Schema::String => reason::tool::Schema::String {
                                        description: Some(param.description.to_owned()),
                                    },
                                    Schema::Integer => reason::tool::Schema::Integer {
                                        description: Some(param.description.to_owned()),
                                    },
                                    Schema::Boolean => reason::tool::Schema::Boolean {
                                        description: Some(param.description.to_owned()),
                                    },
                                },
                            )
                        })
                        .collect(),
                    required: self
                        .parameters
                        .iter()
                        .filter(|param| param.required)
                        .map(|param| param.name.to_owned())
                        .collect(),
                },
            },
        }
    }

    fn new<Arguments: DeserializeOwned + 'static>(
        name: &'static str,
        description: &'static str,
        parameters: &'static [Parameter],
        run: fn(&Path, Arguments) -> Future,
    ) -> Self {
        Self {
            name,
            description,
            parameters,
            run: Box::new(
                move |project, json| match serde_json::from_str::<Arguments>(json) {
                    Ok(arguments) => run(project, arguments),
                    Err(error) => {
                        log::error!("tool arguments failed to parse: {error}");

                        Box::pin(async move {
                            Err(std::io::Error::other(format!(
                                "tool arguments failed to parse: {error}"
                            )))?
                        })
                    }
                },
            ),
        }
    }
}

#[derive(Debug, Clone)]
struct Parameter {
    pub name: &'static str,
    pub description: &'static str,
    pub schema: Schema,
    pub required: bool,
}

#[derive(Debug, Clone)]
enum Schema {
    String,
    Integer,
    Boolean,
}
