//! A fixed-size, payload-free indication of one or more failed hook writes.
//! This is deliberately a lower bound, not an exact queue or an audit log.
use super::*;

fn marker(path: &Path) -> PathBuf {
    path.with_extension("loss")
}

pub(super) fn record(path: &Path, source: ObservationSource) -> Result<()> {
    // Atomic journal replacement permits a bounded read without taking the busy
    // stripe lock. Do not create sidecars for missing/invalid/foreign journals.
    let snapshot = read_journal(&mut open_journal(path)?)?;
    if snapshot.source != Some(source) {
        bail!("observation source mismatch");
    }
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    match options.open(marker(path)) {
        Ok(file) => {
            file.sync_all()?;
            #[cfg(unix)]
            File::open(path.parent().context("invalid journal directory")?)?.sync_all()?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            has_marker(path)?;
        }
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

fn has_marker(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(marker(path)) {
        Ok(metadata) if metadata.is_file() && metadata.len() == 0 => Ok(true),
        Ok(_) => bail!("invalid observation loss marker"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

pub(super) fn annotate(
    path: &Path,
    mut snapshot: HarnessObservations,
) -> Result<HarnessObservations> {
    if has_marker(path)? {
        let mut totals = snapshot.counts();
        totals.discarded_inputs = totals
            .discarded_inputs
            .saturating_add(1)
            .min(9_007_199_254_740_991);
        snapshot.totals = Some(totals);
        snapshot.status = Status::Partial;
    }
    Ok(snapshot)
}

pub(super) fn remove(path: &Path) -> Result<()> {
    match fs::remove_file(marker(path)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}
