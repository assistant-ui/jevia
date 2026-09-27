use super::*;

fn program(path: &Path) {
    fs::write(path, "not launched").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }
}

#[test]
fn executable_lookup_checks_absolute_files_directories_and_missing_paths() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let file = root.join("agent with spaces.exe");
    program(&file);
    assert_eq!(
        lookup(file.to_str().unwrap(), root, None, cfg!(windows)),
        Lookup::Candidate { batch: false }
    );
    assert_eq!(
        lookup(root.to_str().unwrap(), root, None, cfg!(windows)),
        Lookup::NotExecutable
    );
    assert_eq!(
        lookup("missing", root, Some(root.as_os_str()), cfg!(windows)),
        Lookup::NotFound
    );
    for value in ["", "  ", "nul\0command", "directory/"] {
        assert_eq!(
            lookup(value, root, None, cfg!(windows)),
            Lookup::InvalidCommand
        );
    }
    assert_eq!(
        lookup("agent", root, None, cfg!(windows)),
        Lookup::PathUnavailable
    );
    assert_eq!(
        lookup("agent", root, Some(OsStr::new("")), cfg!(windows)),
        Lookup::PathUnavailable
    );
}

#[test]
fn executable_lookup_handles_path_order_spaces_and_windows_suffixes_without_pathext() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let one = root.join("first path");
    let two = root.join("second path");
    fs::create_dir(&one).unwrap();
    fs::create_dir(&two).unwrap();
    let path = env::join_paths([&one, &two]).unwrap();
    program(&one.join("agent.cmd"));
    assert_eq!(lookup("agent", root, Some(&path), true), Lookup::NotFound);
    assert_eq!(
        lookup("agent.cmd", root, Some(&path), true),
        Lookup::Candidate { batch: true }
    );
    program(&two.join("agent.exe"));
    assert_eq!(
        lookup("agent", root, Some(&path), true),
        Lookup::Candidate { batch: false }
    );
    assert_eq!(
        lookup("agent.exe", root, Some(&path), true),
        Lookup::Candidate { batch: false }
    );
    assert_eq!(
        lookup(two.join("agent").to_str().unwrap(), root, None, true),
        Lookup::Candidate { batch: false }
    );
    // An explicit .cmd path tries .cmd.exe first, matching Rust's Windows behavior.
    program(&one.join("agent.cmd.exe"));
    assert_eq!(
        lookup(one.join("agent.cmd").to_str().unwrap(), root, None, true),
        Lookup::Candidate { batch: false }
    );
    fs::create_dir(one.join("agent.exe")).unwrap();
    assert_eq!(
        lookup("agent", root, Some(&path), true),
        Lookup::NotExecutable
    );
}

#[test]
fn windows_relative_paths_are_reported_as_ambiguous() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(
        lookup("./agent.exe", dir.path(), None, true),
        Lookup::RelativeWindowsPath
    );
    assert_eq!(
        lookup("agent", dir.path(), Some(OsStr::new("bin")), true),
        Lookup::RelativeWindowsPath
    );
}

#[test]
#[cfg(unix)]
fn unix_lookup_uses_project_relative_paths_and_executable_bits_without_shell_expansion() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::create_dir(root.join("bin")).unwrap();
    let file = root.join("bin/agent");
    program(&file);
    assert_eq!(
        lookup("./bin/agent", root, None, false),
        Lookup::Candidate { batch: false }
    );
    assert_eq!(
        lookup("agent", root, Some(OsStr::new("bin")), false),
        Lookup::Candidate { batch: false }
    );
    symlink(&file, root.join("link")).unwrap();
    assert_eq!(
        lookup("./link", root, None, false),
        Lookup::Candidate { batch: false }
    );
    fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(
        lookup("./bin/agent", root, None, false),
        Lookup::NotExecutable
    );
    program(&root.join("agent"));
    assert_eq!(
        lookup("agent", root, Some(OsStr::new("bin:")), false),
        Lookup::Candidate { batch: false }
    );
    assert_eq!(
        lookup("agent --version", root, Some(root.as_os_str()), false),
        Lookup::NotFound
    );
    assert_eq!(
        lookup("$AGENT", root, Some(root.as_os_str()), false),
        Lookup::NotFound
    );
}
