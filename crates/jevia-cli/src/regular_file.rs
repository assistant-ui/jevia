//! Open a regular input without blocking on a FIFO substituted at the path.
use anyhow::{Context, Result, bail};
use std::{
    fs::{File, OpenOptions},
    path::Path,
};

pub fn open_optional(path: &Path) -> Result<Option<File>> {
    match std::fs::metadata(path) {
        Ok(metadata) if !metadata.is_file() => bail!("input must be a regular file"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).context("could not inspect regular input"),
        _ => {}
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // Metadata alone is racy: a FIFO may replace the checked regular file.
        options.custom_flags(nix::libc::O_NONBLOCK);
    }
    let file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).context("could not open regular input"),
    };
    if !file.metadata()?.is_file() {
        bail!("input must be a regular file");
    }
    Ok(Some(file))
}
