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
}
