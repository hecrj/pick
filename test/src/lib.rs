use std::fs;
use std::io;
use std::ops::Deref;
use std::path::{Path, PathBuf};

pub struct Directory(PathBuf);

impl Directory {
    pub fn create(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref();

        if fs::exists(path)? {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "test directory already exists",
            ));
        }

        fs::create_dir_all(path)?;

        Ok(Self(path.to_path_buf()))
    }
}

impl AsRef<Path> for Directory {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl Deref for Directory {
    type Target = Path;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
