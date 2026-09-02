use super::{Call, Future};
use crate::file;

use serde::Deserialize;
use tokio::io::AsyncReadExt;

use std::borrow::Cow;
use std::path::Path;

/// The buffer a read refills as it scans the file.
const READ_CHUNK_SIZE: usize = 64 * 1024;
/// How many lines a read can return when the call omits a `limit`.
const DEFAULT_READ_LIMIT: u64 = 1_000;
/// The most lines a caller may request; a larger `limit` is clamped to it.
const MAX_READ_LIMIT: u64 = 10_000;
/// How much of the file a read is willing to scan to find the requested
/// lines; bounds the work, not the output, which is capped by
/// `MAX_OUTPUT_BYTES`.
const MAX_READ_BYTES: u64 = (READ_CHUNK_SIZE as u64) * 4;
/// How much line content a read may return, so a single tool result
/// cannot consume unbounded context.
const MAX_OUTPUT_BYTES: u64 = READ_CHUNK_SIZE as u64;

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
            let _lock = file::lock(&path).await;
            let mut file = tokio::fs::File::open(&path).await?;

            #[derive(Debug, PartialEq)]
            enum Stop {
                EndOfFile,
                LineLimit { has_more: bool },
                ByteLimit,
                OutputLimit,
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

                let mut consumed = 0;

                while consumed < n {
                    let byte = chunk[consumed];
                    consumed += 1;

                    if byte == b'\n' {
                        terminated_lines += 1;

                        if terminated_lines >= offset {
                            if pending.last() == Some(&b'\r') {
                                pending.pop();
                            }

                            let line = String::from_utf8_lossy(&pending);

                            if (output.len() + line.len() + 1) as u64 > MAX_OUTPUT_BYTES {
                                // This line would push the output past
                                // the limit; stop before emitting it, so
                                // the output stays within the limit.
                                stop = Stop::OutputLimit;
                                break;
                            }

                            output.push_str(&line);
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

                if matches!(stop, Stop::OutputLimit) {
                    break;
                }

                if emitted == limit {
                    // The file's continuation may already sit in the
                    // current chunk; only peek one more chunk from the
                    // file if the buffer is exhausted.
                    stop = Stop::LineLimit {
                        has_more: consumed < n || file.read(&mut chunk).await? > 0,
                    };
                    break;
                }

                if scanned >= MAX_READ_BYTES {
                    // A full chunk was scanned; the file may have ended
                    // exactly at the boundary, so peek once to find out.
                    stop = if file.read(&mut chunk).await? > 0 {
                        Stop::ByteLimit
                    } else {
                        Stop::EndOfFile
                    };
                    break;
                }
            }

            // A final line may not end with a newline.
            if matches!(stop, Stop::EndOfFile) && !pending.is_empty() {
                if pending.last() == Some(&b'\r') {
                    pending.pop();
                }

                let line = String::from_utf8_lossy(&pending);

                if (output.len() + line.len() + 1) as u64 > MAX_OUTPUT_BYTES {
                    // The final line would push the output past the limit.
                    stop = Stop::OutputLimit;
                } else if terminated_lines + 1 >= offset {
                    output.push_str(&line);
                    output.push('\n');
                    emitted += 1;
                }

                terminated_lines += 1;
            }

            if output.is_empty() {
                return Ok(match stop {
                    Stop::EndOfFile => {
                        if terminated_lines == 0 {
                            "[File is empty]".to_owned()
                        } else {
                            format!(
                                "[File has {} line{}; offset {} is past the end]",
                                terminated_lines,
                                if terminated_lines == 1 { "" } else { "s" },
                                offset
                            )
                        }
                    }
                    Stop::OutputLimit => format!(
                        "[The first line in range exceeds the {MAX_OUTPUT_BYTES} \
                        byte output limit; inspect the file with bash]"
                    ),
                    _ => format!(
                        "[No complete lines could be read after scanning {scanned} \
                        bytes; the file's lines may be very long]"
                    ),
                });
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
                Stop::OutputLimit => {
                    let out = output.len();

                    Ok(format!(
                        "{output}\n[Read stopped after {out} bytes of output \
                        (limit {MAX_OUTPUT_BYTES}); continue reading with \
                        offset={}, or inspect the file with bash]",
                        offset + emitted
                    ))
                }
                Stop::ByteLimit => Ok(format!(
                    "{output}\n[Read stopped after scanning {scanned} bytes; \
                    some lines may be very long. Continue reading with \
                    offset={}, or inspect the file with bash]",
                    offset + emitted
                )),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{Call, MAX_OUTPUT_BYTES, MAX_READ_BYTES, READ_CHUNK_SIZE, Read};
    use std::path::{Path, PathBuf};

    /// Creates a temporary project directory holding the given files.
    fn project(test: &str, files: &[(&str, &str)]) -> PathBuf {
        let root = std::env::temp_dir()
            .join(format!("pick-read-test-{}", std::process::id()))
            .join(test);

        std::fs::create_dir_all(&root).unwrap();

        for (name, contents) in files {
            std::fs::write(root.join(name), contents).unwrap();
        }

        root
    }

    async fn read(path: &str, project: &Path) -> String {
        read_at(path, project, None, None).await
    }

    async fn read_at(
        path: &str,
        project: &Path,
        offset: Option<u64>,
        limit: Option<u64>,
    ) -> String {
        let read = Read {
            path: path.to_owned(),
            offset,
            limit,
        };

        read.run(project).await.unwrap()
    }

    #[tokio::test]
    async fn reads_small_files_in_full() {
        let root = project("small", &[("small.txt", "a\nb\nc"), ("empty.txt", "")]);

        assert_eq!(read("small.txt", &root).await, "a\nb\nc\n");
        assert_eq!(read("empty.txt", &root).await, "[File is empty]");

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[tokio::test]
    async fn a_file_that_fits_in_a_chunk_is_read_in_full() {
        // ~56 KiB of ordinary lines: large, but smaller than a chunk,
        // so the whole file is returned without any truncation notice.
        let mut contents = String::new();

        for _ in 0..800 {
            contents.push_str(&"x".repeat(69));
            contents.push('\n');
        }

        let root = project("fits", &[("fitting.txt", &contents)]);
        let output = read("fitting.txt", &root).await;

        assert_eq!(output, contents);

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[tokio::test]
    async fn a_file_of_exactly_one_chunk_is_read_in_full() {
        // The file ends exactly at the chunk boundary, so the peek that
        // disambiguates the byte limit must see end of file.
        let mut contents = String::new();

        for _ in 0..(READ_CHUNK_SIZE / 128) {
            contents.push_str(&"y".repeat(127));
            contents.push('\n');
        }

        let root = project("exact", &[("exact.txt", &contents)]);
        let output = read("exact.txt", &root).await;

        assert_eq!(output, contents);

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[tokio::test]
    async fn the_output_limit_applies_to_the_final_line() {
        // 500 short lines followed by a very long line that ends without
        // a newline. The whole file fits in the scan budget, but the
        // final line would push the output past the limit.
        let short: String = (0..500).map(|line| format!("line {line}\n")).collect();
        let contents = format!("{short}{}", "x".repeat(100 * 1024));

        let root = project("final-line", &[("mixed.txt", &contents)]);
        let output = read("mixed.txt", &root).await;

        assert!(output.starts_with("line 0\n"));
        assert!(output.contains("line 499\n"));
        assert!(output.ends_with(&format!(
            "[Read stopped after {} bytes of output \
            (limit {MAX_OUTPUT_BYTES}); continue reading with \
            offset=501, or inspect the file with bash]",
            short.len()
        )));
        // No part of the very long line leaks into the output.
        assert!(!output.contains('x'));

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[tokio::test]
    async fn the_scan_budget_stops_the_read_of_a_huge_line() {
        // 500 short lines followed by a line far larger than the scan
        // budget: the read gives up scanning and reports the bytes it
        // actually saw.
        let short: String = (0..500).map(|line| format!("line {line}\n")).collect();
        let contents = format!("{short}{}", "x".repeat(500 * 1024));

        let root = project("scan-budget", &[("mixed.txt", &contents)]);
        let output = read("mixed.txt", &root).await;

        assert!(output.starts_with("line 0\n"));
        assert!(output.contains("line 499\n"));
        assert!(output.ends_with(&format!(
            "[Read stopped after scanning {MAX_READ_BYTES} bytes; \
            some lines may be very long. Continue reading with \
            offset=501, or inspect the file with bash]"
        )));
        assert!(!output.contains('x'));

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[tokio::test]
    async fn the_output_limit_caps_long_lines() {
        // 900 lines of 100 bytes: emitting 656 of them would push the
        // output past the limit, so the read stops at 655 lines.
        let mut contents = String::new();

        for _ in 0..900 {
            contents.push_str(&"x".repeat(99));
            contents.push('\n');
        }

        let root = project("output-limit", &[("long.txt", &contents)]);
        let output = read("long.txt", &root).await;

        let notice = format!(
            "[Read stopped after 65500 bytes of output \
            (limit {MAX_OUTPUT_BYTES}); continue reading with \
            offset=656, or inspect the file with bash]"
        );

        assert!(output.ends_with(&notice));
        // 655 lines of 100 bytes, plus the newline before the notice.
        assert_eq!(output.len(), 655 * 100 + 1 + notice.len());

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[tokio::test]
    async fn a_deep_offset_reads_past_the_output_limit() {
        // 1200 lines of 100 bytes: reaching line 700 requires scanning
        // past the output limit, which the larger scan budget allows.
        let mut contents = String::new();

        for line in 1..=1200 {
            contents.push_str(&format!("line-{:04}", line));
            contents.push_str(&"x".repeat(90));
            contents.push('\n');
        }

        let root = project("deep-offset", &[("deep.txt", &contents)]);
        let output = read_at("deep.txt", &root, Some(700), None).await;

        // Lines 700 through 1200 are returned in full, with no notice.
        assert!(output.starts_with("line-0700"));
        assert!(output.ends_with(&format!("{}\n", "x".repeat(90))));
        assert!(!output.contains('['));
        assert_eq!(output.len(), 501 * 100);

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[tokio::test]
    async fn a_single_huge_line_yields_a_no_complete_lines_notice() {
        let root = project("no-lines", &[("minified.txt", &"x".repeat(500 * 1024))]);
        let output = read("minified.txt", &root).await;

        assert_eq!(
            output,
            format!(
                "[No complete lines could be read after scanning {MAX_READ_BYTES} \
                bytes; the file's lines may be very long]"
            )
        );

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[tokio::test]
    async fn a_single_oversized_line_yields_an_output_limit_notice() {
        // A single line bigger than the output limit but smaller than
        // the scan budget: the whole file is scanned and end of file is
        // reached, the final-line flush rejects the line, and nothing
        // is emitted. (The 500 KiB fixture above stops at the scan
        // budget before reaching end of file instead.)
        let root = project(
            "single-oversized",
            &[("oversized.txt", &"x".repeat(100 * 1024))],
        );
        let output = read("oversized.txt", &root).await;

        assert_eq!(
            output,
            format!(
                "[The first line in range exceeds the {MAX_OUTPUT_BYTES} \
                byte output limit; inspect the file with bash]"
            )
        );

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[tokio::test]
    async fn a_small_line_before_an_oversized_line_yields_an_output_limit_notice() {
        // A short line followed by a line bigger than the output limit
        // (but smaller than the scan budget): the whole file is scanned
        // and end of file is reached, so the short line is returned and
        // the final line is rejected by the flush.
        let small = "hello\n";
        let contents = format!("{small}{}", "x".repeat(100 * 1024));

        let root = project("oversized-line", &[("oversized.txt", &contents)]);
        let output = read("oversized.txt", &root).await;

        assert_eq!(
            output,
            format!(
                "{small}\n[Read stopped after {} bytes of output \
                (limit {MAX_OUTPUT_BYTES}); continue reading with \
                offset=2, or inspect the file with bash]",
                small.len()
            )
        );

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[tokio::test]
    async fn line_limit_advises_a_continuation_offset() {
        let contents: String = (1..=1500).map(|line| format!("line {line}\n")).collect();

        let root = project("line-limit", &[("many.txt", &contents)]);
        let output = read("many.txt", &root).await;

        assert!(output.starts_with("line 1\n"));
        assert!(output.contains("line 1000\n"));
        assert!(output.ends_with("[File has more lines; continue reading with offset=1001]"));

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[tokio::test]
    async fn line_limit_peek_detects_continuation_beyond_the_buffer() {
        // The 1000th line ends exactly at the end of the first 64 KiB
        // chunk, so the continuation can only be seen by peeking one
        // more chunk from the file.
        let mut contents = String::new();

        for _ in 0..999 {
            contents.push_str(&"a".repeat(63));
            contents.push('\n');
        }

        contents.push_str(&"b".repeat(READ_CHUNK_SIZE - 999 * 64 - 1));
        contents.push('\n');

        for _ in 0..10 {
            contents.push_str(&"c".repeat(63));
            contents.push('\n');
        }

        let root = project("peek", &[("boundary.txt", &contents)]);
        let output = read("boundary.txt", &root).await;

        assert!(output.ends_with("[File has more lines; continue reading with offset=1001]"));

        std::fs::remove_dir_all(&root).unwrap();
    }
}
