//! Private, best-effort fairness hint. Never stores event data or changes history.
use super::*;

pub(super) struct Cursor {
    path: PathBuf,
    _guard: crate::lease::FileLock,
}

impl Cursor {
    pub(super) fn open(directory: &Path) -> Result<Self> {
        crate::ensure_local_ignore(&directory.join(".gitignore"))?;
        let state = directory.join("replay-state");
        if let Ok(metadata) = fs::symlink_metadata(&state)
            && !metadata.is_dir()
        {
            bail!("invalid replay state directory");
        }
        let guard = crate::lease::try_acquire(&state, "cursor")?
            .context("another replay owns the cursor")?;
        Ok(Self {
            path: state.join("cursor"),
            _guard: guard,
        })
    }

    pub(super) fn read(&self) -> Result<Option<String>> {
        match fs::symlink_metadata(&self.path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Ok(metadata) if metadata.is_file() && metadata.len() == 36 => {}
            _ => bail!("invalid replay cursor"),
        }
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(nix::libc::O_NOFOLLOW);
        }
        let mut text = String::new();
        options
            .open(&self.path)?
            .take(37)
            .read_to_string(&mut text)?;
        uuid::Uuid::parse_str(&text).context("invalid replay cursor")?;
        Ok(Some(text))
    }

    pub(super) fn advance(&self, id: &str) -> Result<()> {
        uuid::Uuid::parse_str(id)?;
        let mut temp = tempfile::NamedTempFile::new_in(self.path.parent().unwrap())?;
        temp.write_all(id.as_bytes())?;
        temp.persist(&self.path).map_err(|error| error.error)?;
        // Atomic visibility is sufficient for a retry-order hint. Losing it after
        // power failure restarts the cycle; no observation is deleted or credited.
        Ok(())
    }
}
