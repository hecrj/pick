pub use std::path::{Path, PathBuf};

pub fn tildify(path: &Path, home: Option<&Path>) -> PathBuf {
    let Some(home) = home else {
        return path.to_path_buf();
    };

    match path.strip_prefix(home) {
        Ok(rest) if rest.as_os_str().is_empty() => PathBuf::from("~"),
        Ok(rest) => Path::new("~").join(rest),
        Err(_) => path.to_path_buf(),
    }
}
