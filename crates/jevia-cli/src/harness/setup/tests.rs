use super::*;
use jevia_core::Config;
use std::fs;

fn options() -> Options {
    Options {
        name: "agent".into(),
        command: "my-agent".into(),
        args: vec![
            "run".into(),
            "--model".into(),
            "{model}".into(),
            "{task}".into(),
        ],
        models: Config::default()
            .tiers
            .keys()
            .map(|tier| (tier.clone(), format!("provider/{tier}")))
            .collect(),
        verify_command: Some("cargo".into()),
        verify_args: vec!["test".into()],
        no_verification: false,
        apply: false,
        replace: false,
    }
}

fn fixture() -> (tempfile::TempDir, ProjectPaths) {
    let directory = tempfile::tempdir().unwrap();
    let paths = ProjectPaths::at(directory.path().into());
    fs::create_dir(&paths.directory).unwrap();
    fs::write(&paths.config, Config::default().to_toml().unwrap()).unwrap();
    (directory, paths)
}

#[test]
fn harness_edits_preserve_ordinary_inline_dotted_tables_and_unrelated_policy() {
    let other = "command = 'other', args = ['{model}', '{task}'], models = {fast='a', balanced='b', strong='c'}";
    for prefix in [
        String::new(),
        format!("harnesses = {{other = {{{other}}}}}\n"),
        format!("harnesses.other = {{{other}}}\n"),
    ] {
        let (_directory, paths) = fixture();
        let base = Config::default()
            .to_toml()
            .unwrap()
            .replace("[harnesses]\n", "");
        let original = format!("# keep this comment\n{prefix}{base}");
        fs::write(&paths.config, &original).unwrap();
        let first = prepare(&paths, &options()).unwrap();
        assert_eq!(fs::read_to_string(&paths.config).unwrap(), original);
        assert!(first.rendered.contains("# keep this comment"));
        assert_eq!(Config::from_toml(&first.rendered).unwrap(), first.next);
        let mut without_selected = first.next.clone();
        without_selected.harnesses.remove("agent");
        assert_eq!(without_selected, first.previous);
        first.commit().unwrap();
        let repeated = prepare(&paths, &options()).unwrap();
        assert_eq!(repeated.rendered, first.rendered);
        assert_eq!(repeated.previous, repeated.next);
    }
}

#[test]
fn replacement_preserves_verifier_unless_explicitly_changed_or_removed() {
    let (_directory, paths) = fixture();
    let first = prepare(&paths, &options()).unwrap();
    first.commit().unwrap();
    let verifier = first.next.harnesses["agent"].verification.clone();
    let mut changed = options();
    changed.command = "new-agent".into();
    changed.verify_command = None;
    changed.verify_args.clear();
    assert_eq!(
        prepare(&paths, &changed).unwrap().next.harnesses["agent"].verification,
        verifier
    );
    changed.no_verification = true;
    assert!(
        prepare(&paths, &changed).unwrap().next.harnesses["agent"]
            .verification
            .is_none()
    );
}

#[test]
fn invalid_harness_values_are_rejected_without_echoing_private_arguments() {
    let (_directory, paths) = fixture();
    let original = fs::read(&paths.config).unwrap();
    for case in 0..8 {
        let mut invalid = options();
        match case {
            0 => {
                invalid.models.pop();
            }
            1 => invalid.models.push(invalid.models[0].clone()),
            2 => invalid
                .models
                .push(("unknown".into(), "private-model".into())),
            3 => invalid.args.push("{private-unknown-placeholder}".into()),
            4 => invalid.args = vec!["{model}".into()],
            5 => invalid.name = "private-\x1b[31m".into(),
            6 => invalid
                .verify_args
                .push("{private-verifier-placeholder}".into()),
            7 => invalid.command = "private-\0command".into(),
            _ => unreachable!(),
        }
        let error = prepare(&paths, &invalid).err().unwrap();
        assert!(!format!("{error:#}").contains("private-"));
        assert_eq!(fs::read(&paths.config).unwrap(), original);
    }
    assert_eq!(
        model_mapping("fast=model=version").unwrap(),
        ("fast".into(), "model=version".into())
    );
    for bad in ["fast", "=model", "fast=", "fast=  "] {
        assert!(model_mapping(bad).is_err());
    }
}

#[test]
fn concurrent_edit_and_shared_setup_lock_prevent_overwrites() {
    let (_directory, paths) = fixture();
    let edit = prepare(&paths, &options()).unwrap();
    let concurrent = format!("# concurrent\n{}", edit.original);
    fs::write(&paths.config, &concurrent).unwrap();
    assert!(edit.commit().is_err());
    assert_eq!(fs::read_to_string(&paths.config).unwrap(), concurrent);
    let lock = config_lock(&paths).unwrap();
    let mut apply = options();
    apply.apply = true;
    assert!(run(&paths, apply).is_err());
    drop(lock);
}
