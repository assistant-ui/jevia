//! Deliberately conservative filesystem/PATH inspection, not a process launcher.
use std::{
    env,
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Lookup {
    Candidate { batch: bool },
    InvalidCommand,
    NotFound,
    NotExecutable,
    PathUnavailable,
    RelativeWindowsPath,
}

pub(super) fn lookup(program: &str, root: &Path, path: Option<&OsStr>, windows: bool) -> Lookup {
    if program.trim().is_empty()
        || program.contains('\0')
        || program.ends_with('/')
        || (windows && program.ends_with('\\'))
    {
        return Lookup::InvalidCommand;
    }
    let program_path = Path::new(program);
    let bare = program_path.file_name() == Some(OsStr::new(program));
    if !bare {
        if windows && !program_path.is_absolute() {
            return Lookup::RelativeWindowsPath;
        }
        let candidate = root.join(program_path);
        if windows && !program.to_ascii_lowercase().ends_with(".exe") {
            // Rust tries appending .exe for explicit non-.exe paths before the
            // exact path. Never use set_extension: preserve existing suffixes.
            let mut exe = candidate.as_os_str().to_owned();
            exe.push(".exe");
            let result = inspect(&PathBuf::from(exe), windows);
            if result != Lookup::NotFound {
                return result;
            }
        }
        return inspect(&candidate, windows);
    }
    let Some(path) = path.filter(|path| !path.is_empty()) else {
        return Lookup::PathUnavailable;
    };
    let name = if windows && !program.contains('.') {
        format!("{program}.exe")
    } else {
        program.to_owned()
    };
    let mut rejected = false;
    for directory in env::split_paths(path) {
        if windows && directory.as_os_str().is_empty() {
            continue;
        }
        if windows && !directory.is_absolute() {
            return Lookup::RelativeWindowsPath;
        }
        match inspect(&root.join(directory).join(&name), windows) {
            found @ Lookup::Candidate { .. } => return found,
            Lookup::NotFound => {}
            // Windows can stop at an existing invalid candidate; a later PATH
            // entry must not hide that failure in the static check.
            Lookup::NotExecutable if windows => return Lookup::NotExecutable,
            _ => rejected = true,
        }
    }
    if rejected {
        Lookup::NotExecutable
    } else {
        Lookup::NotFound
    }
}

fn inspect(path: &Path, windows: bool) -> Lookup {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Lookup::NotFound,
        Err(_) => return Lookup::NotExecutable,
    };
    if !metadata.is_file() {
        return Lookup::NotExecutable;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if !windows && metadata.permissions().mode() & 0o111 == 0 {
            return Lookup::NotExecutable;
        }
    }
    let batch = windows
        && path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("cmd") || ext.eq_ignore_ascii_case("bat"));
    Lookup::Candidate { batch }
}

#[cfg(test)]
mod tests;
