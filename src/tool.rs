pub mod bash;
pub mod call;
pub mod edit;
pub mod read;
pub mod write;

use bash::Bash;
use edit::Edit;
use read::Read;
use write::Write;

pub use crate::core::Output;
pub use call::Call;

use iced::{Color, color};
use serde::de::DeserializeOwned;

use std::collections::BTreeMap;

/// The background of the tool view
pub const BACKGROUND: Color = color!(0x111);

pub struct Tool {
    name: &'static str,
    description: &'static str,
    parameters: &'static [Parameter],
    parse: Box<dyn Fn(&str) -> Result<Box<dyn Call>, reason::Error>>,
}

impl Tool {
    /// The builtin tools, in a stable order: the metadata is
    /// serialized into every request, where it forms part of the
    /// prefix the server's prompt cache matches, so the order must
    /// not vary from process to process.
    pub fn builtins() -> BTreeMap<&'static str, Self> {
        let tools = [
            Self::new::<Read>(
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
            ),
            Self::new::<Bash>(
                "bash",
                "Run a bash command",
                &[
                    Parameter {
                        name: "command",
                        description: "Command to run",
                        schema: Schema::String,
                        required: true,
                    },
                    Parameter {
                        name: "description",
                        description: "A short 3-5 word summary of the command",
                        schema: Schema::String,
                        required: false,
                    },
                ],
            ),
            Self::new::<Write>(
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
            ),
            Self::new::<Edit>(
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
            ),
        ];

        BTreeMap::from_iter(tools.into_iter().map(|tool| (tool.name, tool)))
    }

    pub fn parse(&self, arguments: &str) -> Result<Box<dyn Call>, reason::Error> {
        (self.parse)(arguments)
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

    fn new<C: Call + DeserializeOwned + 'static>(
        name: &'static str,
        description: &'static str,
        parameters: &'static [Parameter],
    ) -> Self {
        Self {
            name,
            description,
            parameters,
            parse: Box::new(move |json| match serde_json::from_str::<C>(json) {
                Ok(call) => Ok(Box::new(call)),
                Err(error) => {
                    log::warn!("tool arguments failed to parse: {error}");

                    Err(std::io::Error::other(format!(
                        "tool arguments failed to parse: {error}"
                    )))?
                }
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
    Integer,
    Boolean,
}

#[cfg(test)]
mod tests {
    use super::Tool;

    /// The builtins come out in the same order in every process;
    /// the tools are serialized into the prefix of every request,
    /// where the server's prompt cache matches them, so a varying
    /// order would miss the cache on every restart.
    #[test]
    fn the_builtins_are_in_a_stable_order() {
        assert_eq!(
            Tool::builtins().keys().copied().collect::<Vec<_>>(),
            ["bash", "edit", "read", "write"]
        );
    }
}
