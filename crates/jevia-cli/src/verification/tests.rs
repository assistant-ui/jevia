use super::*;
use serde_json::json;
use std::fs;

fn plan(root: &Path) -> VerificationInvocation {
    let Detection::Found(plan) = detect(root) else {
        panic!("expected detected verifier")
    };
    plan
}

#[test]
fn discovers_rust_and_respects_declared_node_package_managers() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("Cargo.toml"), "[workspace]\nmembers=[]\n").unwrap();
    assert_eq!(plan(dir.path()).program, "cargo");
    assert_eq!(plan(dir.path()).args, ["test", "--workspace"]);
    fs::remove_file(dir.path().join("Cargo.toml")).unwrap();
    for manager in ["npm", "pnpm", "yarn", "bun"] {
        fs::write(dir.path().join("package.json"), json!({"packageManager": format!("{manager}@1.0.0"), "scripts": {"test": "test-runner"}}).to_string()).unwrap();
        let expected = if cfg!(windows) && manager != "bun" {
            format!("{manager}.cmd")
        } else {
            manager.into()
        };
        assert_eq!(plan(dir.path()).program, expected);
        assert_eq!(plan(dir.path()).args, ["run", "test"]);
    }
}

#[test]
fn uses_unambiguous_lockfiles_and_defaults_to_npm_without_installing_anything() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("package.json"),
        r#"{"scripts":{"test":"test-runner"}}"#,
    )
    .unwrap();
    assert!(plan(dir.path()).program.starts_with("npm"));
    for (file, manager) in [
        ("pnpm-lock.yaml", "pnpm"),
        ("yarn.lock", "yarn"),
        ("bun.lockb", "bun"),
        ("npm-shrinkwrap.json", "npm"),
    ] {
        fs::write(dir.path().join(file), "").unwrap();
        assert!(plan(dir.path()).program.starts_with(manager));
        fs::remove_file(dir.path().join(file)).unwrap();
    }
    fs::write(dir.path().join("pnpm-lock.yaml"), "").unwrap();
    fs::write(dir.path().join("yarn.lock"), "").unwrap();
    assert!(matches!(detect(dir.path()), Detection::Unavailable(_)));
}

#[test]
fn never_guesses_for_missing_placeholder_ambiguous_or_invalid_manifests() {
    let dir = tempfile::tempdir().unwrap();
    assert!(matches!(detect(dir.path()), Detection::Unavailable(_)));
    let package = dir.path().join("package.json");
    for test in [
        "",
        " ",
        "true",
        "exit 0",
        ":",
        "echo \"Error: no test specified\" && exit 1",
    ] {
        fs::write(&package, json!({"scripts": {"test": test}}).to_string()).unwrap();
        assert!(matches!(detect(dir.path()), Detection::Unavailable(_)));
    }
    for raw in [
        "PRIVATE_INVALID".into(),
        "x".repeat(MAX_MANIFEST_BYTES as usize + 1),
        json!({"packageManager": "PRIVATE_COMMAND@1", "scripts": {"test": "anything"}}).to_string(),
    ] {
        fs::write(&package, raw).unwrap();
        let Detection::Unavailable(message) = detect(dir.path()) else {
            panic!("invalid manifest accepted")
        };
        assert!(!message.contains("PRIVATE"));
    }
    fs::write(&package, r#"{"scripts":{"test":"test-runner"}}"#).unwrap();
    fs::write(dir.path().join("Cargo.toml"), "[package]\nname='test'\n").unwrap();
    assert!(matches!(detect(dir.path()), Detection::Unavailable(_)));
}

#[cfg(unix)]
#[test]
fn refuses_symlink_manifests_without_reading_the_target() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("private-target"), "PRIVATE_CONTENT").unwrap();
    std::os::unix::fs::symlink(
        dir.path().join("private-target"),
        dir.path().join("package.json"),
    )
    .unwrap();
    let Detection::Unavailable(message) = detect(dir.path()) else {
        panic!("symlink accepted")
    };
    assert!(!message.contains("PRIVATE"));
}
