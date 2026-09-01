use super::{Call, Future};

use serde::Deserialize;
use tokio::io::AsyncReadExt;

use std::borrow::Cow;
use std::path::Path;

const READ_CHUNK_SIZE: usize = 64 * 1024;
const DEFAULT_READ_LIMIT: u64 = 1_000;
const MAX_READ_LIMIT: u64 = 10_000;
const MAX_READ_BYTES: u64 = 50 * 1024;

#[derive(Deserialize)]
pub struct Read {
    path: String,
    #[serde(default)]
    offset: Option<u64>,
    #[serde(default)]
    limit: Option<u64>,
}

impl Call for Read {
    fn title(&self) -> Option<Cow<'_, str>> {
        let offset = match (self.offset, self.limit) {
            (None, None) => String::new(),
            (Some(offset), None) => format!(" ({offset}..)"),
            (None, Some(limit)) => format!(" (..{limit})"),
            (Some(offset), Some(limit)) => format!(" ({offset}..{end})", end = offset + limit),
        };

        Some(format!("{}{offset}", self.path).into())
    }

    fn run(&self, project: &Path) -> Future {
        let path = self.path.clone();
        let offset = self.offset;
        let limit = self.limit;
        let project = project.to_path_buf();

        Box::pin(async move {
            let offset = match offset {
                Some(offset) if offset > 0 => offset,
                Some(_) => Err(std::io::Error::other(
                    "offset must be a 1-based line number (>= 1)",
                ))?,
                None => 1,
            };

            let limit = match limit {
                Some(limit) if limit > 0 => limit.min(MAX_READ_LIMIT),
                Some(_) => Err(std::io::Error::other("limit must be >= 1"))?,
                None => DEFAULT_READ_LIMIT,
            };

            let path = project.join(&path);
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
}
