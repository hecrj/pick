use iced::Task;

use serde::Deserialize;
use serde::de::DeserializeOwned;

use std::collections::HashMap;
use std::path::Path;

type Result = ::core::result::Result<String, reason::Error>;

pub struct Tool {
    name: &'static str,
    description: &'static str,
    parameters: &'static [Parameter],
    run: Box<dyn Fn(&Path, &str) -> Task<Result>>,
}

impl Tool {
    pub fn builtins() -> HashMap<&'static str, Self> {
        #[derive(Deserialize)]
        struct Bash {
            command: String,
        }

        fn bash(project: &Path, arguments: Bash) -> Task<Result> {
            let project = project.to_path_buf();

            Task::future(async move {
                let output = tokio::process::Command::new("bash")
                    .args(["-c", &arguments.command])
                    .current_dir(project)
                    .output()
                    .await?;

                if output.status.success() {
                    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
                } else {
                    Err(std::io::Error::other(
                        String::from_utf8_lossy(&output.stderr).into_owned(),
                    ))?
                }
            })
        }

        #[derive(Deserialize)]
        struct Read {
            path: String,
        }

        fn read(project: &Path, arguments: Read) -> Task<Result> {
            let project = project.to_path_buf();

            Task::future(async move {
                let contents = tokio::fs::read_to_string(project.join(arguments.path)).await?;

                Ok(contents)
            })
        }

        #[derive(Deserialize)]
        struct Write {
            path: String,
            content: String,
        }

        fn write(project: &Path, arguments: Write) -> Task<Result> {
            let project = project.to_path_buf();

            Task::future(async move {
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

        fn edit(project: &Path, arguments: Edit) -> Task<Result> {
            let project = project.to_path_buf();

            Task::future(async move {
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
                &[Parameter {
                    name: "path",
                    description: "Path of the file",
                    schema: Schema::String,
                    required: true,
                }],
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
                "Edit a file by replacing an exact string match. old_string must match exactly one location in the file, including whitespace and indentation, unless replace_all is set",
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

    pub fn run(&self, project: impl AsRef<Path>, arguments: &str) -> Task<Result> {
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
        run: fn(&Path, Arguments) -> Task<Result>,
    ) -> Self {
        Self {
            name,
            description,
            parameters,
            run: Box::new(move |project, json| {
                let Ok(arguments) = serde_json::from_str(json) else {
                    log::error!("tool arguments failed to parse!");

                    return Task::none(); // TODO
                };

                run(project, arguments)
            }),
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
    Boolean,
}
