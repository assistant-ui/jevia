//! Shared preview/backup/atomic replacement for explicit configuration editors.
use crate::paths::ProjectPaths;
use anyhow::{Context, Result, anyhow, bail};
use jevia_core::Config;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};
use toml_edit::DocumentMut;

pub(crate) fn config_lock(paths: &ProjectPaths) -> Result<fs::File> {
    let mut options = fs::OpenOptions::new();
    options.create(true).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let lock = options.open(paths.directory.join("config.lock"))?;
    match lock.try_lock() {
        Ok(()) => Ok(lock),
        Err(fs::TryLockError::WouldBlock) => {
            bail!("another configuration setup is running; retry after it finishes")
        }
        Err(fs::TryLockError::Error(error)) => {
            Err(error).context("could not lock configuration for setup")
        }
    }
}

pub(crate) struct ConfigEdit {
    path: PathBuf,
    pub(crate) original: String,
    pub(crate) rendered: String,
    permissions: fs::Permissions,
    pub(crate) previous: Config,
    pub(crate) next: Config,
}

impl ConfigEdit {
    pub(crate) fn prepare(
        path: &Path,
        edit: impl FnOnce(&mut Config, &mut DocumentMut) -> Result<()>,
    ) -> Result<Self> {
        let metadata = fs::symlink_metadata(path)?;
        if !metadata.is_file() {
            bail!("setup requires a regular config.toml, not a symlink");
        }
        let original = fs::read_to_string(path)?;
        let previous = Config::from_toml(&original).map_err(|_| {
            anyhow!("invalid project configuration; setup refused (contents redacted)")
        })?;
        let mut next = previous.clone();
        let mut document = original
            .parse::<DocumentMut>()
            .map_err(|_| anyhow!("could not edit project configuration (contents redacted)"))?;
        edit(&mut next, &mut document)?;
        next.validate().map_err(|_| {
            anyhow!("invalid proposed configuration; setup refused (contents redacted)")
        })?;
        let rendered = if next == previous {
            original.clone()
        } else {
            document.to_string()
        };
        if Config::from_toml(&rendered).ok().as_ref() != Some(&next) {
            bail!("edited configuration did not preserve project settings; setup refused");
        }
        Ok(Self {
            path: path.to_owned(),
            original,
            rendered,
            permissions: metadata.permissions(),
            previous,
            next,
        })
    }

    pub(crate) fn ensure_unchanged(&self) -> Result<()> {
        if !fs::symlink_metadata(&self.path)?.is_file()
            || fs::read_to_string(&self.path)? != self.original
        {
            bail!("configuration changed during setup; refusing to overwrite concurrent edits");
        }
        Ok(())
    }

    pub(crate) fn backup(&self, paths: &ProjectPaths) -> Result<PathBuf> {
        let directory = paths.directory.join("config-backups");
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(&directory)?;
        let mut snapshot = tempfile::Builder::new()
            .prefix("config-")
            .suffix(".toml")
            .tempfile_in(&directory)?;
        snapshot.write_all(self.original.as_bytes())?;
        snapshot.as_file().sync_all()?;
        let (_, path) = snapshot.keep().map_err(|e| e.error)?;
        sync_directory(&directory)?;
        sync_directory(&paths.directory)?;
        Ok(path)
    }

    pub(crate) fn commit(&self) -> Result<()> {
        let directory = self
            .path
            .parent()
            .context("configuration path has no parent")?;
        let mut temporary = tempfile::Builder::new()
            .prefix(".config-")
            .suffix(".tmp")
            .tempfile_in(directory)?;
        temporary.write_all(self.rendered.as_bytes())?;
        temporary
            .as_file()
            .set_permissions(self.permissions.clone())?;
        temporary.as_file().sync_all()?;
        self.ensure_unchanged()?;
        temporary.persist(&self.path).map_err(|e| e.error)?;
        sync_directory(directory)
    }
}

fn sync_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    fs::File::open(path)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}
