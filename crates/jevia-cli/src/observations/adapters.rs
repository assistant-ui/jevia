use super::*;

const CODEX_EVENTS: &[&str] = &[
    "SessionStart",
    "SessionEnd",
    "UserPromptSubmit",
    "Stop",
    "Interrupt",
    "PostToolUse",
    "SubagentStart",
    "SubagentStop",
];
const OPENCODE_EVENTS: &[&str] = &[
    "SessionStart",
    "UserPromptSubmit",
    "Stop",
    "StopFailure",
    "PostToolUse",
    "PostToolUseFailure",
    "SubagentStart",
    "ModelObserved",
];

pub(super) fn events(source: ObservationSource) -> &'static [&'static str] {
    match source {
        ObservationSource::ClaudeHooks => HOOKS,
        ObservationSource::CodexHooks => CODEX_EVENTS,
        ObservationSource::OpencodePlugin => OPENCODE_EVENTS,
        ObservationSource::Application => &[],
    }
}

pub(super) fn conflicts(source: ObservationSource, args: &[String]) -> bool {
    match source {
        ObservationSource::Application => true,
        ObservationSource::ClaudeHooks => args.iter().any(|a| {
            matches!(a.as_str(), "--settings" | "--bare" | "--safe-mode")
                || a.starts_with("--settings=")
        }),
        // Do not replace explicit session config or hook-disable preferences.
        // Remote/daemon hooks cannot safely use this launch's private environment.
        // Windows shell quoting is not part of the tested Codex adapter contract yet.
        ObservationSource::CodexHooks => {
            cfg!(windows)
                || args.iter().any(|a| {
                    matches!(
                        a.as_str(),
                        "-c" | "--config"
                            | "--disable"
                            | "--remote"
                            | "app-server"
                            | "daemon"
                            | "attach"
                    ) || a.starts_with("--config=")
                        || a.starts_with("--remote=")
                        || a.starts_with("--disable=")
                        || a.starts_with("-c=")
                })
        }
        ObservationSource::OpencodePlugin => args.iter().any(|a| {
            matches!(
                a.as_str(),
                "--attach" | "attach" | "serve" | "web" | "--pure"
            ) || a.starts_with("--attach=")
        }),
    }
}

pub(super) fn version_supported(source: ObservationSource, output: &str) -> bool {
    let version = match source {
        ObservationSource::CodexHooks => output
            .strip_prefix("codex-cli ")
            .and_then(|s| s.split_whitespace().next()),
        _ => output.split_whitespace().next(),
    };
    let Some(version) = version else { return false };
    // This alpha is the locally verified hook/no-daemon contract. Unknown
    // prereleases and future Codex minors must not silently acquire new flags.
    if source == ObservationSource::CodexHooks && version == "0.158.0-alpha.2" {
        return true;
    }
    let Ok(parts) = version
        .split('.')
        .map(str::parse::<u32>)
        .collect::<std::result::Result<Vec<_>, _>>()
    else {
        return false;
    };
    let [major, minor, patch] = parts.as_slice() else {
        return false;
    };
    match source {
        // 2.1.212 was exercised with real SessionStart, Read, Stop and
        // SessionEnd hooks. Do not exclude it merely because newer event
        // kinds were added to the adapter later.
        ObservationSource::ClaudeHooks => *major == 2 && [*minor, *patch] >= [1, 212],
        ObservationSource::CodexHooks => *major == 0 && *minor == 158,
        ObservationSource::OpencodePlugin => *major == 1 && [*minor, *patch] >= [18, 33],
        ObservationSource::Application => false,
    }
}

pub(super) struct Installation {
    pub args: Vec<String>,
    pub environment: Vec<(String, String)>,
    pub plugin: Option<tempfile::TempPath>,
}

pub(super) fn install(
    source: ObservationSource,
    executable: &Path,
    journal: &Path,
    directory: &Path,
) -> Result<Installation> {
    let mut result = Installation {
        args: vec![],
        environment: vec![],
        plugin: None,
    };
    match source {
        ObservationSource::Application => bail!("application recording is not a native adapter"),
        ObservationSource::ClaudeHooks => {
            let mut hooks = serde_json::Map::new();
            for name in HOOKS {
                hooks.insert((*name).into(), json!([{"hooks": [{"type": "command", "command": executable, "args": ["capture-event", "--journal", journal], "timeout": 2}]}]));
            }
            result.args = vec![
                "--settings".into(),
                serde_json::to_string(&json!({"hooks": hooks}))?,
            ];
        }
        ObservationSource::CodexHooks => {
            // A stable command permits normal trust review to survive later runs.
            // The private path comes from the isolated child's environment, never
            // a shell expansion or a project/user configuration file.
            let executable = executable.to_str().context("non-UTF8 executable path")?;
            let command = format!(
                "'{}' capture-event --source codex_hooks",
                executable.replace('\'', "'\"'\"'")
            );
            result.args.push("--no-daemon".into());
            for event in CODEX_EVENTS {
                // Cold process startup can exceed two seconds on a busy host.
                // Codex caps SessionEnd/Interrupt at three seconds.
                let timeout = if matches!(*event, "SessionEnd" | "Interrupt") {
                    3
                } else {
                    5
                };
                let value = format!(
                    "hooks.{event}=[{{hooks=[{{type=\"command\",command={},timeout={timeout}}}]}}]",
                    serde_json::to_string(&command)?
                );
                result.args.extend(["-c".into(), value]);
            }
            result.environment.push((
                JOURNAL_ENV.into(),
                journal.to_str().context("non-UTF8 journal path")?.into(),
            ));
        }
        ObservationSource::OpencodePlugin => {
            let existing = match std::env::var("OPENCODE_CONFIG_CONTENT") {
                Ok(value) => Some(value),
                Err(std::env::VarError::NotPresent) => None,
                Err(error) => return Err(error.into()),
            };
            // Parse before creating an auxiliary file; malformed/JSONC custom
            // overrides remain untouched, with process-only recording.
            let mut config = inline_config(existing.as_deref())?;
            let mut plugin = tempfile::Builder::new()
                .prefix(&crate::recordings::asset_prefix("jevia-observer-", journal))
                .suffix(".mjs")
                .tempfile_in(directory)?;
            plugin.write_all(include_bytes!("opencode.mjs"))?;
            plugin.as_file().sync_all()?;
            let url = url::Url::from_file_path(plugin.path())
                .map_err(|_| anyhow::anyhow!("invalid plugin path"))?;
            config["plugin"]
                .as_array_mut()
                .context("invalid plugin list")?
                .push(Value::String(url.to_string()));
            result.environment = vec![
                (
                    JOURNAL_ENV.into(),
                    journal.to_str().context("non-UTF8 journal path")?.into(),
                ),
                (
                    "JEVIA_OBSERVATION_EXECUTABLE".into(),
                    executable
                        .to_str()
                        .context("non-UTF8 executable path")?
                        .into(),
                ),
                (
                    "OPENCODE_CONFIG_CONTENT".into(),
                    serde_json::to_string(&config)?,
                ),
            ];
            result.plugin = Some(plugin.into_temp_path());
        }
    }
    Ok(result)
}

fn inline_config(existing: Option<&str>) -> Result<Value> {
    let mut config: Value = match existing {
        Some(value) => serde_json::from_str(value)?,
        None => json!({}),
    };
    let object = config.as_object_mut().context("invalid inline config")?;
    let plugins = object.entry("plugin").or_insert_with(|| json!([]));
    if !plugins.is_array() {
        bail!("invalid plugin list");
    }
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[cfg(unix)]
    async fn auto_detection_installs_native_adapters_only_for_supported_versions() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        for (name, version, source) in [
            (
                "codex",
                "codex-cli 0.158.0-alpha.2",
                ObservationSource::CodexHooks,
            ),
            ("opencode", "1.18.33", ObservationSource::OpencodePlugin),
        ] {
            let program = directory.path().join(name);
            fs::write(&program, format!("#!/bin/sh\nprintf '{version}\\n'\n")).unwrap();
            fs::set_permissions(&program, fs::Permissions::from_mode(0o700)).unwrap();
            let invocation = HarnessInvocation {
                program: program.to_str().unwrap().into(),
                args: vec!["--".into(), "task".into()],
                model: "requested".into(),
                verification: None,
            };
            let id = uuid::Uuid::new_v4().to_string();
            let captured =
                Capture::prepare(ObservationMode::Auto, &invocation, directory.path(), &id).await;
            assert_eq!(captured.snapshot().source, Some(source));
            assert_eq!(captured.snapshot().status, Status::NoEvents);
            assert_eq!(captured.args.last().unwrap(), "task");
            assert_eq!(captured.args[captured.args.len() - 2], "--");
            captured.persisted(&captured.snapshot());
            if source == ObservationSource::CodexHooks {
                let exec = HarnessInvocation {
                    args: vec![
                        "exec".into(),
                        "--model".into(),
                        "requested".into(),
                        "task".into(),
                    ],
                    ..invocation.clone()
                };
                let capture =
                    Capture::prepare(ObservationMode::Auto, &exec, directory.path(), &id).await;
                assert_eq!(capture.snapshot().status, Status::NoEvents);
                assert!(!capture.args.iter().any(|arg| arg == "--no-daemon"));
                assert_eq!(capture.args.first().unwrap(), "exec");
                capture.persisted(&capture.snapshot());
            }
            fs::write(&program, "#!/bin/sh\nprintf '0.0.0\\n'\n").unwrap();
            let unavailable =
                Capture::prepare(ObservationMode::Auto, &invocation, directory.path(), &id).await;
            assert_eq!(unavailable.snapshot().status, Status::Unavailable);
            assert_eq!(unavailable.args, invocation.args);
            assert!(unavailable.environment.is_empty());
            let disabled =
                Capture::prepare(ObservationMode::Off, &invocation, directory.path(), &id).await;
            assert_eq!(disabled.snapshot().status, Status::Disabled);
            assert!(disabled.environment.is_empty());
        }
    }

    #[test]
    fn native_payloads_are_allowlisted_without_success_inference() {
        let raw = json!({"hook_event_name":"PostToolUse", "model":"actual-model", "session_id":"s1", "tool_name":"Bash", "tool_response":"PRIVATE_RESULT", "prompt":"PRIVATE_PROMPT"});
        for source in [
            ObservationSource::CodexHooks,
            ObservationSource::OpencodePlugin,
        ] {
            let event = normalize(source, &raw).unwrap();
            assert_eq!(event.kind, Kind::ToolCompleted);
            assert_eq!(event.model.as_deref(), Some("actual-model"));
            assert!(!serde_json::to_string(&event).unwrap().contains("PRIVATE_"));
            assert!(normalize(source, &json!({"hook_event_name":"PostModelSwitch"})).is_none());
        }
        assert_eq!(
            normalize(ObservationSource::ClaudeHooks, &raw)
                .unwrap()
                .kind,
            Kind::ToolSucceeded
        );
        assert_eq!(
            normalize(
                ObservationSource::CodexHooks,
                &json!({"hook_event_name":"Interrupt"})
            )
            .unwrap()
            .kind,
            Kind::TurnInterrupted
        );
    }

    #[tokio::test]
    async fn adapter_journals_reject_other_sources_and_remain_isolated() {
        let directory = tempfile::tempdir().unwrap();
        let invocation = HarnessInvocation {
            program: "compatible-wrapper".into(),
            args: vec![],
            model: "requested".into(),
            verification: None,
        };
        let first = Capture::prepare(
            ObservationMode::OpencodePlugin,
            &invocation,
            directory.path(),
            &uuid::Uuid::new_v4().to_string(),
        )
        .await;
        let second = Capture::prepare(
            ObservationMode::OpencodePlugin,
            &invocation,
            directory.path(),
            &uuid::Uuid::new_v4().to_string(),
        )
        .await;
        assert_eq!(first.initial.status, Status::NoEvents);
        assert_ne!(first.journal, second.journal);
        let input = br#"{"hook_event_name":"ModelObserved","model":"provider/actual"}"#;
        assert!(
            receive_for_source(
                first.journal.as_ref().unwrap(),
                ObservationSource::ClaudeHooks,
                input.as_slice()
            )
            .is_err()
        );
        receive_for_source(
            first.journal.as_ref().unwrap(),
            ObservationSource::OpencodePlugin,
            input.as_slice(),
        )
        .unwrap();
        assert_eq!(first.snapshot().event_count(), 1);
        assert_eq!(second.snapshot().event_count(), 0);
        assert_eq!(
            second.snapshot().source,
            Some(ObservationSource::OpencodePlugin)
        );
        first.persisted(&first.snapshot());
        second.persisted(&second.snapshot());
        assert!(first.auxiliary.iter().all(|path| !path.exists()));
    }

    #[test]
    fn remote_and_custom_settings_remain_untouched() {
        for arg in [
            "--config",
            "-c",
            "--config=hooks",
            "--remote=ws://host",
            "--disable=hooks",
        ] {
            assert!(conflicts(ObservationSource::CodexHooks, &[arg.into()]));
        }
        for arg in ["attach", "serve", "--attach=http://host", "--pure"] {
            assert!(conflicts(ObservationSource::OpencodePlugin, &[arg.into()]));
        }
    }

    #[test]
    fn version_gates_are_conservative() {
        for version in ["2.1.212 (Claude Code)", "2.1.251", "2.1.287"] {
            assert!(version_supported(ObservationSource::ClaudeHooks, version));
        }
        for version in ["2.1.211", "3.0.0", "2.1.212-beta", "PRIVATE_OUTPUT"] {
            assert!(!version_supported(ObservationSource::ClaudeHooks, version));
        }
        for version in [
            "codex-cli 0.158.0-alpha.2",
            "codex-cli 0.158.0",
            "codex-cli 0.158.1",
        ] {
            assert!(version_supported(ObservationSource::CodexHooks, version));
        }
        for version in [
            "codex-cli 0.157.0",
            "codex-cli 0.159.0",
            "codex-cli 0.158.0-alpha.1",
            "0.158.0",
            "PRIVATE_OUTPUT",
        ] {
            assert!(!version_supported(ObservationSource::CodexHooks, version));
        }
        assert!(version_supported(
            ObservationSource::OpencodePlugin,
            "1.18.33"
        ));
        for version in ["1.18.32", "2.0.0", "1.18.33-beta", "PRIVATE_OUTPUT"] {
            assert!(!version_supported(
                ObservationSource::OpencodePlugin,
                version
            ));
        }
    }

    #[test]
    fn inline_config_preserves_other_settings_and_plugins() {
        let config = inline_config(Some(
            r#"{"model":"other/model","plugin":["existing"],"permission":{"bash":"ask"}}"#,
        ))
        .unwrap();
        assert_eq!(config["plugin"], json!(["existing"]));
        assert_eq!(config["permission"]["bash"], "ask");
        assert!(inline_config(Some("{// JSONC\n}")).is_err());
        assert!(inline_config(Some("[]")).is_err());
        assert!(inline_config(Some(r#"{"plugin":false}"#)).is_err());
    }

    #[test]
    fn hook_commands_are_stable_quoted_and_valid_toml() {
        let dir = tempfile::tempdir().unwrap();
        let executable = Path::new("/test dir/it's-$(not-executed)/jevia");
        let one = install(
            ObservationSource::CodexHooks,
            executable,
            Path::new("/a"),
            dir.path(),
        )
        .unwrap();
        let two = install(
            ObservationSource::CodexHooks,
            executable,
            Path::new("/b"),
            dir.path(),
        )
        .unwrap();
        assert_eq!(one.args, two.args);
        assert_ne!(one.environment, two.environment);
        for config in one.args.iter().skip(2).step_by(2) {
            config.parse::<toml_edit::DocumentMut>().unwrap();
            assert!(!config.contains("--journal"));
            assert!(!config.contains("bypass"));
            if config.starts_with("hooks.SessionEnd=") || config.starts_with("hooks.Interrupt=") {
                assert!(config.contains("timeout=3"));
            } else {
                assert!(config.contains("timeout=5"));
            }
        }
    }
}
