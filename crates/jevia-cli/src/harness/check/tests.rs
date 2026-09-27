use super::*;
use jevia_core::HarnessConfig;

fn fixture() -> (tempfile::TempDir, ProjectPaths, Config) {
    let directory = tempfile::tempdir().unwrap();
    let paths = ProjectPaths::at(directory.path().into());
    fs::create_dir(&paths.directory).unwrap();
    let mut config = Config::default();
    config.harnesses.insert(
        "agent".into(),
        HarnessConfig {
            command: env::current_exe().unwrap().to_str().unwrap().into(),
            args: vec!["{model}".into(), "{task}".into()],
            models: config
                .tiers
                .keys()
                .map(|tier| (tier.clone(), "private-model".into()))
                .collect(),
            verification: None,
        },
    );
    fs::write(&paths.config, config.to_toml().unwrap()).unwrap();
    (directory, paths, config)
}

#[test]
fn preflight_reports_missing_harness_and_config_failures_without_leaking_contents() {
    let (_dir, paths, _config) = fixture();
    let report = inspect(&paths, "absent\x1b[31m");
    assert!(!report.ok);
    assert!(
        report
            .checks
            .iter()
            .any(|check| check.code == "harness_not_configured")
    );
    assert!(!report.render().contains('\x1b'));
    // Build deliberately invalid config without passing Config's serializer validation.
    let valid = fs::read_to_string(&paths.config).unwrap();
    fs::write(&paths.config, valid.replace("{task}", "{private-unknown}")).unwrap();
    let report = inspect(&paths, "agent");
    assert!(!report.ok);
    assert_eq!(report.checks[0].code, "invalid_template");
    assert!(!report.render().contains("private-"));
    fs::write(&paths.config, "private-malformed = [").unwrap();
    let report = inspect(&paths, "agent");
    assert_eq!(report.checks[0].code, "invalid_configuration");
    assert!(!serde_json::to_string(&report).unwrap().contains("private-"));
}

#[test]
fn preflight_warns_without_verifier_and_rejects_nul_arguments() {
    let (_dir, paths, mut config) = fixture();
    let report = inspect(&paths, "agent");
    assert!(report.ok);
    assert!(report.checks.iter().any(
        |check| check.code == "verification_not_configured" && check.status == Status::Warning
    ));
    assert!(!report.render().contains("private-model"));
    config
        .harnesses
        .get_mut("agent")
        .unwrap()
        .models
        .insert("fast".into(), "private-\0model".into());
    fs::write(&paths.config, config.to_toml().unwrap()).unwrap();
    let report = inspect(&paths, "agent");
    assert!(!report.ok);
    assert!(
        report
            .checks
            .iter()
            .any(|check| check.code == "invalid_arguments")
    );
    assert!(!serde_json::to_string(&report).unwrap().contains("private-"));
}
