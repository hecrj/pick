//! Sandboxing the process.
//!
//! The model runs arbitrary commands through the `bash` tool and
//! may pass absolute paths to the file tools, so before the
//! runtime starts, [`enter`] re-executes the process under a
//! sandbox in which:
//!
//! - the sandboxed process has no host identity and cannot see or
//!   signal host processes;
//! - the root filesystem is read-only, with the project, the
//!   project's data directory, the user's projects directory, and
//!   a scratchpad under the host's temp directory as the only
//!   writable places besides the ssh-agent socket;
//! - the home directory is hidden, with only the Rust toolchain
//!   remounted;
//! - the environment is cleared down to `PATH`, `HOME`, `USER`,
//!   the ssh-agent socket variable, the display variables the GUI
//!   needs, the host's locale variables, and piolet's own `PIOLET_*`
//!   configuration, so tokens the user exported never reach the
//!   model's shell.
//!
//! Only Linux is supported today, where the sandbox is a
//! bubblewrap invocation; other platforms get [`Error::Unsupported`].
//!
//! [`enter`] is called once from `main`. When the re-execution
//! succeeds it does not return: the process image is replaced, and
//! the new process re-enters `main` with the `PIOLET_SANDBOXED`
//! environment variable set to `1`, where it becomes a no-op. The
//! check is on the value, not the variable's presence, so a
//! `PIOLET_SANDBOXED` the host environment happens to set to anything
//! but `1` cannot skip the sandbox. When a sandbox cannot be
//! started, it reports why, so `main` can panic rather than keep
//! running bare; the `--we-doin-it-live` flag skips the sandbox
//! entirely.
use crate::Project;

use std::env;
use std::fmt;

#[cfg(target_os = "linux")]
mod linux;

/// The environment marker identifying an already-sandboxed process,
/// so the re-executed process does not sandbox itself again.
const MARKER: &str = "PIOLET_SANDBOXED";

/// The value the re-execution sets the marker to, and the only
/// value [`already_sandboxed`] accepts: a marker the host
/// environment sets to anything else is not the re-execution's.
const MARKER_VALUE: &str = "1";

/// Re-executes the process under a sandbox in which `project`,
/// the directory the tools operate in, the project's data
/// directory, and the user's projects directory are the only
/// writable places, besides the scratchpad under the host's temp
/// directory.
///
/// See the module documentation for the policy.
pub fn enter(_project: &Project) -> Result<(), Error> {
    if already_sandboxed() {
        return Ok(());
    }

    #[cfg(target_os = "linux")]
    {
        linux::enter(_project)
    }

    #[cfg(not(target_os = "linux"))]
    {
        Err(Error::Unsupported)
    }
}

/// Why a process runs without a sandbox.
#[allow(dead_code)]
pub enum Error {
    /// Bubblewrap only exists on Linux.
    Unsupported,
    /// No `bwrap` binary was found on the `PATH`.
    Missing,
    /// The probe failed; the detail says why, typically user
    /// namespaces being disabled.
    Probe(String),
    /// The project directory could not be resolved.
    Project,
    /// The project's data directory could not be created.
    DataDir(String),
    /// The scratchpad under the host's temp directory could not
    /// be prepared.
    Scratch(String),
    /// The re-execution failed for some other reason.
    Exec(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Unsupported => f.write_str("bubblewrap is only available on Linux"),
            Self::Missing => f.write_str("no `bwrap` binary on the PATH; install bubblewrap"),
            Self::Probe(detail) => {
                if detail.trim().is_empty() {
                    f.write_str("the sandbox probe failed (user namespaces may be disabled)")
                } else {
                    write!(f, "the sandbox probe failed: {detail}")
                }
            }
            Self::Project => f.write_str("the project directory could not be resolved"),
            Self::DataDir(detail) => {
                write!(
                    f,
                    "the project's data directory could not be created: {detail}"
                )
            }
            Self::Scratch(detail) => {
                write!(f, "the scratchpad could not be prepared: {detail}")
            }
            Self::Exec(detail) => write!(f, "the sandbox re-execution failed: {detail}"),
        }
    }
}

/// Whether this process is the sandboxed re-execution, which the
/// re-execution marks by setting the marker to [`MARKER_VALUE`].
/// The check is on the value, not the variable's presence, so a
/// `PIOLET_SANDBOXED` the host environment happens to set — a leak
/// from debugging, say — cannot skip the sandbox.
fn already_sandboxed() -> bool {
    env::var_os(MARKER).is_some_and(|value| value.as_os_str() == MARKER_VALUE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_marker_only_counts_at_its_own_value() {
        // The test binary is not sandboxed, so start from the
        // variable being absent and leave it absent.
        unsafe {
            env::remove_var(MARKER);
        }
        assert!(!already_sandboxed());

        unsafe {
            env::set_var(MARKER, "0");
        }
        assert!(!already_sandboxed());

        unsafe {
            env::set_var(MARKER, MARKER_VALUE);
        }
        assert!(already_sandboxed());

        unsafe {
            env::remove_var(MARKER);
        }
        assert!(!already_sandboxed());
    }
}
