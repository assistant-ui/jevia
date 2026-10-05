use std::collections::BTreeMap;

use anyhow::{Context, Result, bail};
use clap::{Args, ValueEnum};
use jevia_core::{HarnessConfig, HarnessInvocation, VerificationConfig};
use toml_edit::{Array, DocumentMut, Item, Table, Value, value};

use crate::{
    config_edit::{ConfigEdit, config_lock},
    paths::ProjectPaths,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Preset {
    /// OpenAI Codex CLI in non-interactive exec mode.
    Codex,
    /// Anthropic Claude Code in non-interactive print mode.
    Claude,
    /// OpenCode in non-interactive run mode.
    Opencode,
    /// Google Gemini CLI in non-interactive prompt mode.
    Gemini,
}

impl Preset {
    fn command(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::Opencode => "opencode",
            Self::Gemini => "gemini",
        }
    }

    fn args(self) -> &'static [&'static str] {
        match self {
            Self::Codex => &["exec", "--model", "{model}", "{task}"],
            Self::Claude => &["--print", "--model", "{model}", "{task}"],
            Self::Opencode => &["run", "--model", "{model}", "{task}"],
            Self::Gemini => &["--model", "{model}", "--prompt", "{task}"],
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::Opencode => "opencode",
            Self::Gemini => "gemini",
        }
    }
}

/// Separate every prompt from native options for the exact built-in templates.
/// Even a plain word like `attach` must not be mistaken for a capture conflict.
/// Custom wrappers/templates keep their argv contract; never guess their parser.
pub fn preserve_literal_task(
    harness: &HarnessConfig,
    invocation: &mut HarnessInvocation,
    task: &str,
) {
    let name = std::path::Path::new(&harness.command)
        .file_name()
        .and_then(|name| name.to_str());
    let Some(preset) = Preset::value_variants().iter().find(|preset| {
        name.is_some_and(|name| {
            name == preset.command() || name == format!("{}.exe", preset.command())
        }) && harness.args == preset.args()
    }) else {
        return;
    };
    // Gemini already uses an explicit prompt option and has no native adapter.
    // Only leading-dash values need its existing equals-form protection.
    if *preset == Preset::Gemini && !task.starts_with('-') {
        return;
    }
    let task_index = harness.args.len() - 1;
    invocation.args.remove(task_index);
    if *preset == Preset::Gemini {
        invocation.args[task_index - 1] = format!("--prompt={task}");
    } else {
        // Extra native options still precede the task delimiter. Injected
        // recording options will also be inserted before this delimiter.
        if !invocation.args.iter().any(|arg| arg == "--") {
            invocation.args.push("--".into());
        }
        invocation.args.push(task.into());
    }
}

#[derive(Debug, Args)]
pub struct Options {
    /// Local adapter name (letters, digits, hyphens, underscores).
    name: String,
    /// Use a built-in command template; model IDs remain explicit.
    #[arg(long, value_enum, conflicts_with_all = ["command", "args"])]
    preset: Option<Preset>,
    /// Executable name or path, not a shell command. Never put credentials here.
    #[arg(long, required_unless_present = "preset")]
    command: Option<String>,
    /// Repeat in execution order; use --arg=--flag for leading hyphens.
    /// Include {model} and {task}. Nothing is shell-expanded.
    #[arg(
        long = "arg",
        value_name = "ARG",
        required_unless_present = "preset",
        conflicts_with = "preset"
    )]
    args: Vec<String>,
    /// Repeat once per configured tier, e.g. --model fast=provider/small.
    #[arg(long = "model", value_name = "TIER=MODEL", required = true, value_parser = model_mapping)]
    models: Vec<(String, String)>,
    /// Optional verifier executable; omission preserves an existing verifier.
    #[arg(long, conflicts_with_all = ["no_verification", "auto_verification"])]
    verify_command: Option<String>,
    /// Repeat verifier arguments; use --verify-arg=--flag for leading hyphens.
    #[arg(long = "verify-arg", value_name = "ARG", requires = "verify_command")]
    verify_args: Vec<String>,
    /// Disable both explicit and automatic verification (process-only records).
    #[arg(long, conflicts_with = "verify_command")]
    no_verification: bool,
    /// Opt in to automatic project tests instead of an explicit verifier.
    #[arg(long, conflicts_with_all = ["no_verification", "verify_command"])]
    auto_verification: bool,
    /// Back up and atomically save the previewed harness configuration.
    #[arg(long)]
    apply: bool,
    /// Allow --apply to change an existing harness. Unrelated settings are preserved.
    #[arg(long)]
    replace: bool,
}

impl Options {
    fn command_template(&self) -> Result<(String, Vec<String>)> {
        if let Some(preset) = self.preset {
            if self.command.is_some() || !self.args.is_empty() {
                bail!("--preset cannot be combined with --command or --arg");
            }
            return Ok((
                preset.command().into(),
                preset.args().iter().map(|arg| (*arg).into()).collect(),
            ));
        }

        let command = self
            .command
            .clone()
            .context("supply either --preset or --command with at least one --arg")?;
        if self.args.is_empty() {
            bail!("custom harness setup requires at least one --arg");
        }
        Ok((command, self.args.clone()))
    }
}

fn model_mapping(input: &str) -> Result<(String, String), String> {
    let Some((tier, model)) = input.split_once('=') else {
        return Err("expected TIER=MODEL".into());
    };
    if tier.trim().is_empty() || model.trim().is_empty() || input.contains('\0') {
        return Err("tier and model must be nonempty and contain no NUL bytes".into());
    }
    Ok((tier.to_owned(), model.to_owned()))
}

fn prepare(paths: &ProjectPaths, options: &Options) -> Result<ConfigEdit> {
    if options.name.is_empty()
        || !options
            .name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        bail!("harness name must contain only letters, digits, hyphens, and underscores");
    }
    let (command, args) = options.command_template()?;
    if command.trim().is_empty()
        || command.contains('\0')
        || args
            .iter()
            .chain(&options.verify_args)
            .any(|arg| arg.contains('\0'))
        || options
            .verify_command
            .as_ref()
            .is_some_and(|s| s.trim().is_empty() || s.contains('\0'))
    {
        bail!("commands must be nonempty and command/argument values must not contain NUL bytes");
    }
    ConfigEdit::prepare(&paths.config, |next, document| {
        let mut models = BTreeMap::new();
        for (tier, model) in &options.models {
            if !next.tiers.contains_key(tier) {
                bail!("model mapping names an unknown tier; use the tiers in .jevia/config.toml");
            }
            if models.insert(tier.clone(), model.clone()).is_some() {
                bail!("duplicate model mapping; supply each configured tier exactly once");
            }
        }
        let missing: Vec<_> = next
            .tiers
            .keys()
            .filter(|tier| !models.contains_key(*tier))
            .collect();
        if !missing.is_empty() {
            bail!("missing --model mappings for tiers: {missing:?}");
        }
        let previous = next.harnesses.get(&options.name);
        let verification = if let Some(command) = &options.verify_command {
            Some(VerificationConfig {
                command: command.clone(),
                args: options.verify_args.clone(),
            })
        } else if options.no_verification || options.auto_verification {
            None
        } else {
            previous.and_then(|harness| harness.verification.clone())
        };
        let harness = HarnessConfig {
            observations: previous.map(|h| h.observations).unwrap_or_default(),
            auto_verify: if options.no_verification {
                false
            } else if options.auto_verification {
                true
            } else {
                previous.is_some_and(|h| h.auto_verify)
            },
            command,
            args,
            models,
            verification,
        };
        if previous == Some(&harness) {
            return Ok(());
        }
        next.harnesses.insert(options.name.clone(), harness.clone());
        next.validate().map_err(|_| anyhow::anyhow!(
            "invalid harness templates; include {{model}} and {{task}} in --arg values and use only {{model}}, {{task}}, {{tier}}, or {{run_id}} placeholders (contents redacted)"
        ))?;
        // TableLike supports ordinary, dotted, and inline harness tables. Inserting
        // only the selected key preserves unrelated harnesses and policy comments.
        if document.get("harnesses").is_none() {
            document["harnesses"] = Item::Table(Table::new());
        }
        let item = harness_item(&harness);
        let inline = document["harnesses"].is_inline_table();
        let container = document["harnesses"]
            .as_table_like_mut()
            .context("harnesses must be a TOML table")?;
        let item = if inline {
            Item::Value(
                item.into_value()
                    .map_err(|_| anyhow::anyhow!("could not render harness table"))?,
            )
        } else {
            item
        };
        container.insert(&options.name, item);
        Ok(())
    })
}

pub fn print_presets() {
    println!("Built-in harness templates (model IDs are not guessed):");
    for preset in Preset::value_variants() {
        println!(
            "  {:<8} {} {}",
            preset.name(),
            preset.command(),
            preset.args().join(" ")
        );
    }
    println!(
        "Preview one with `jevia harness setup <name> --preset <preset> --model <tier>=<model> ...`."
    );
    println!("Setup never installs or launches the selected harness.");
}

pub fn run(paths: &ProjectPaths, options: Options) -> Result<()> {
    let _lock = if options.apply {
        Some(config_lock(paths)?)
    } else {
        None
    };
    let edit = prepare(paths, &options)?;
    let harness = &edit.next.harnesses[&options.name];
    let changed = edit.previous != edit.next;
    let replacing = edit.previous.harnesses.contains_key(&options.name) && changed;
    let mut snippet = DocumentMut::new();
    snippet["harnesses"] = Item::Table(Table::new());
    snippet["harnesses"][&options.name] = harness_item(harness);
    println!(
        "{} harness setup for {:?}:\n{snippet}",
        if options.apply { "Applying" } else { "Preview" },
        options.name
    );
    if harness.verification.is_none() {
        if harness.auto_verify {
            println!(
                "Automatic project tests are enabled; use harness check to preview detection. No manual feedback is needed after a verified run."
            );
        } else {
            println!(
                "Verification disabled: a successful process exit is not verified learning evidence."
            );
        }
    }
    if !options.apply {
        println!("No files changed, programs launched, or API/database requests made.");
        println!(
            "Review the template and model IDs, then repeat with --apply{}.",
            if replacing { " --replace" } else { "" }
        );
        return Ok(());
    }
    if replacing && !options.replace {
        bail!("harness already exists; review the preview and use --apply --replace to change it");
    }
    if !changed {
        edit.ensure_unchanged()?;
        println!("Harness already matches; configuration unchanged, no backup needed.");
        return Ok(());
    }
    crate::ensure_local_ignore(&paths.directory.join(".gitignore"))?;
    edit.ensure_unchanged()?;
    let backup = edit.backup(paths)?;
    println!("Config backup: {}", crate::terminal::path(&backup));
    edit.commit().context(
        "could not finish harness setup; inspect config and retain its backup before retrying",
    )?;
    println!(
        "Harness saved. No programs were launched; this does not verify executable availability, provider credentials, or model access."
    );
    Ok(())
}

fn arguments(args: &[String]) -> Item {
    value(
        args.iter()
            .map(|arg| Value::from(arg.as_str()))
            .collect::<Array>(),
    )
}

fn harness_item(harness: &HarnessConfig) -> Item {
    let mut table = Table::new();
    table["command"] = value(&harness.command);
    table["args"] = arguments(&harness.args);
    table["auto_verify"] = value(harness.auto_verify);
    table["observations"] = value(harness.observations.as_str());
    let mut models = Table::new();
    for (tier, model) in &harness.models {
        models[tier] = value(model);
    }
    table["models"] = Item::Table(models);
    if let Some(verification) = &harness.verification {
        let mut verify = Table::new();
        verify["command"] = value(&verification.command);
        verify["args"] = arguments(&verification.args);
        table["verification"] = Item::Table(verify);
    }
    Item::Table(table)
}

#[cfg(test)]
mod tests;
