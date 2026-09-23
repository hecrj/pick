use std::env;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    pub id: Id,
    pub path: PathBuf,
    pub home: Option<PathBuf>,
}

impl Project {
    /// Returns the [`Project`] of the current directory, if
    /// available.
    pub fn current_dir() -> io::Result<Self> {
        let path = env::current_dir()?;

        Ok(Self::new(path, env::home_dir()))
    }

    pub(crate) fn new(path: impl AsRef<Path>, home: Option<PathBuf>) -> Self {
        let path = path.as_ref().to_path_buf();

        Self {
            id: Id::new(&path),
            path,
            home,
        }
    }

    pub fn data_dir(&self) -> PathBuf {
        dirs::data_dir()
            .unwrap_or_default()
            .join("pick")
            .join(&self.id)
    }

    pub fn join(&self, path: impl AsRef<Path>) -> PathBuf {
        self.path.join(path)
    }

    pub fn relative(&self, path: impl AsRef<Path>) -> PathBuf {
        let path = path.as_ref();

        match path.strip_prefix(&self.path) {
            Ok(rest) if rest.as_os_str().is_empty() => PathBuf::from("."),
            Ok(rest) => rest.to_path_buf(),
            Err(_) => self.tildify(path),
        }
    }

    fn tildify(&self, path: impl AsRef<Path>) -> PathBuf {
        let path = path.as_ref();

        let Some(home) = &self.home else {
            return path.to_path_buf();
        };

        match path.strip_prefix(home) {
            Ok(rest) if rest.as_os_str().is_empty() => PathBuf::from("~"),
            Ok(rest) => Path::new("~").join(rest),
            Err(_) => path.to_path_buf(),
        }
    }
}

impl AsRef<Path> for Project {
    fn as_ref(&self) -> &Path {
        &self.path
    }
}

impl fmt::Display for Project {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.tildify(&self.path).display())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Id(String);

impl Id {
    fn new(project: impl AsRef<Path>) -> Self {
        /// FNV-1a over a byte slice, the deterministic half of the
        /// scratchpad's name. `DefaultHasher` is randomly seeded per
        /// process, so a std hasher would not name the same leaf across
        /// launches.
        fn fnv1a_64(bytes: &[u8]) -> u64 {
            const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
            const PRIME: u64 = 0x0000_0100_0000_01b3;

            bytes.iter().fold(OFFSET_BASIS, |state, &byte| {
                (state ^ u64::from(byte)).wrapping_mul(PRIME)
            })
        }

        let project = project.as_ref();
        let hash = format!("{:016x}", fnv1a_64(project.as_os_str().as_encoded_bytes()));

        let id = match project.file_name().and_then(|name| name.to_str()) {
            Some(name) => format!("{name}-{}", &hash[..8]),
            None => hash[..8].to_owned(),
        };

        Self(id)
    }
}

impl AsRef<Path> for Id {
    fn as_ref(&self) -> &Path {
        Path::new(&self.0)
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(path: impl AsRef<Path>) -> Id {
        Id::new(path)
    }

    #[test]
    fn the_id_is_deterministic_and_named_after_the_project() {
        // Deterministic: the same project names the same id...
        assert_eq!(id("/home/user/code/pick"), id("/home/user/code/pick"));

        // ...named after the project and a hash of its path...
        let name = id("/home/user/code/pick");
        assert!(name.0.starts_with("pick-"), "{name}");
        assert_eq!(name.0.len(), "pick-".len() + 8);

        // ...and a different project names a different id.
        assert_ne!(id("/home/user/code/pick"), id("/home/user/code/other"));
    }

    #[test]
    fn a_non_utf8_name_falls_back_to_the_hash() {
        // The file name is not UTF-8, so the id is named after the hash alone.
        #[cfg(unix)]
        let path: PathBuf = {
            use std::ffi::OsStr;
            use std::os::unix::ffi::OsStrExt;

            Path::new(OsStr::from_bytes(b"/home/user/code/pick-\xff")).to_path_buf()
        };

        #[cfg(windows)]
        let path: PathBuf = {
            use std::ffi::OsString;
            use std::os::windows::ffi::OsStringExt;

            // An unpaired surrogate is not valid UTF-8, so `to_str`
            // returns `None`, like a non-UTF-8 name on Unix.
            OsString::from_wide(&[0xdc00]).into()
        };

        let name = id(path);

        assert_eq!(name.0.len(), 8);
        assert!(name.0.chars().all(|c| c.is_ascii_hexdigit()), "{name}");
    }

    #[test]
    fn the_id_is_pinned_for_a_known_project() {
        // Golden vector: the id is on-disk state that survives
        // launches, so the naming scheme is frozen — a change
        // would rename every leaf and orphan the scratch it held.
        assert_eq!(id("/home/user/code/pick").0, "pick-30941fe7");
    }

    fn project(path: &str, home: Option<&str>) -> Project {
        Project::new(path, home.map(PathBuf::from))
    }

    /// A path inside the project, including the project itself, is
    /// shown relative to it.
    #[test]
    fn a_path_inside_the_project_is_shown_relative_to_it() {
        let project = project("/home/user/code/pick", Some("/home/user"));

        assert_eq!(project.relative("/home/user/code/pick"), Path::new("."));
        assert_eq!(
            project.relative("/home/user/code/pick/src/main.rs"),
            Path::new("src/main.rs")
        );
    }

    /// A path that merely shares a string prefix with the project is
    /// a different directory, so it is not shown relative to it.
    #[test]
    fn a_sibling_with_a_shared_string_prefix_is_not_relative() {
        let project = project("/home/user/code/pick", Some("/home/user"));

        assert_eq!(
            project.relative("/home/user/code/picker/main.rs"),
            Path::new("~/code/picker/main.rs")
        );
    }

    /// A path outside the project but inside the home directory is
    /// shown with a tilde, including the home directory itself.
    #[test]
    fn a_path_outside_the_project_is_shown_with_a_tilde() {
        let project = project("/home/user/code/pick", Some("/home/user"));

        assert_eq!(project.relative("/home/user"), Path::new("~"));
        assert_eq!(
            project.relative("/home/user/docs/notes.md"),
            Path::new("~/docs/notes.md")
        );
    }

    /// A path outside both the project and the home directory is
    /// shown as is.
    #[test]
    fn a_path_outside_the_project_and_home_is_shown_as_is() {
        let project = project("/home/user/code/pick", Some("/home/user"));

        assert_eq!(project.relative("/etc/hosts"), Path::new("/etc/hosts"));
    }

    /// Without a known home directory there is nothing to tildify,
    /// so a path outside the project is shown as is.
    #[test]
    fn a_path_outside_the_project_is_shown_as_is_without_a_home() {
        let project = project("/home/user/code/pick", None);

        assert_eq!(
            project.relative("/home/user/docs/notes.md"),
            Path::new("/home/user/docs/notes.md")
        );
    }

    /// The project displays as its tildified path, so the status bar
    /// abbreviates a project under the home directory.
    #[test]
    fn the_project_displays_as_its_tildified_path() {
        // The display is a plain `String`, so the separators must be
        // the platform's own; on Windows a `join` produces `\`.
        #[cfg(unix)]
        {
            let project = project("/home/user/code/pick", Some("/home/user"));

            assert_eq!(project.to_string(), "~/code/pick");
        }

        #[cfg(windows)]
        {
            let project = project(r"C:\home\user\code\pick", Some(r"C:\home\user"));

            assert_eq!(project.to_string(), r"~\code\pick");
        }
    }
}
