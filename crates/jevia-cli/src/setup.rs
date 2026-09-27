//! Explicit, preview-first JSONL-to-database setup. Config is switched last;
//! database transactions and filesystem replacement are not one transaction.

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow, bail};
use clap::{Args, Subcommand};
use jevia_core::{Config, StorageConfig};
use toml_edit::{DocumentMut, Item, Table, value};

use crate::{
    paths::ProjectPaths,
    storage::{Storage, validate_import},
    store,
};

#[derive(Debug, Args)]
pub struct Options {
    #[command(subcommand)]
    backend: Backend,
    /// Initialize/check the destination, optionally import, then save config.
    #[arg(long, global = true, requires = "confirm_stopped")]
    apply: bool,
    /// Confirm all Jevia writers/supervisors for this workspace have stopped.
    #[arg(long, global = true, requires = "apply")]
    confirm_stopped: bool,
    /// Import the current JSONL history; required when it contains records.
    #[arg(long, global = true)]
    import_jsonl: bool,
}

#[derive(Debug, Subcommand)]
enum Backend {
    /// Use a local SQLite file (relative paths start at the project root).
    Sqlite {
        #[arg(long, default_value = ".jevia/jevia.db")]
        path: PathBuf,
    },
    /// Use an existing PostgreSQL database; never pass credentials as arguments.
    Postgres {
        #[arg(long, default_value = "JEVIA_DATABASE_URL")]
        url_env: String,
        #[arg(long)]
        project: String,
        /// Development only: allow plaintext to a loopback host.
        #[arg(long)]
        allow_insecure_localhost: bool,
    },
}

impl Backend {
    fn config(&self) -> Result<StorageConfig> {
        Ok(match self {
            Self::Sqlite { path } => StorageConfig::Sqlite {
                url: format!(
                    "sqlite://{}",
                    path.to_str().context("SQLite path must be UTF-8")?
                ),
            },
            Self::Postgres {
                url_env,
                project,
                allow_insecure_localhost,
            } => StorageConfig::Postgres {
                url_env: url_env.clone(),
                project: project.clone(),
                allow_insecure_localhost: *allow_insecure_localhost,
            },
        })
    }
}

pub async fn run(paths: &ProjectPaths, options: Options) -> Result<()> {
    // Serialize setup invocations without claiming to lock editors or already
    // running supervisors. Operator quiescence is an explicit requirement.
    let _lock = if options.apply {
        Some(config_lock(paths)?)
    } else {
        None
    };
    let edit = ConfigEdit::prepare(&paths.config, options.backend.config()?)?;
    if !edit.previous.storage.is_jsonl() && edit.previous.storage != edit.next.storage {
        bail!(
            "setup only supports JSONL-to-database configuration or the same existing target; use explicit export/import for SQL-to-SQL moves"
        );
    }
    if options.import_jsonl && !edit.previous.storage.is_jsonl() {
        bail!(
            "--import-jsonl requires the current backend to be JSONL; old JSONL files may be stale. Use storage import-jsonl explicitly if intended"
        );
    }
    protect_sqlite_target(paths, &edit.next.storage)?;
    let source = if edit.previous.storage.is_jsonl() {
        read_optional(&paths.runs)?
    } else {
        None
    };
    let records = store::parse_snapshot(&paths.runs, source.as_deref().unwrap_or_default())
        .map_err(|_| anyhow!("JSONL history is invalid or unsupported; inspect/repair it before setup (record contents redacted)"))?;
    validate_import(&records)?;
    let changed = edit.previous.storage != edit.next.storage;
    println!(
        "{} database setup:",
        if options.apply { "Applying" } else { "Preview" }
    );
    let mut snippet = DocumentMut::new();
    snippet["storage"] = storage_item(&edit.next.storage);
    print!("{snippet}");
    println!(
        "JSONL records: {}. Import requested: {}.",
        records.len(),
        options.import_jsonl
    );
    if !options.apply {
        if !records.is_empty() && !options.import_jsonl {
            println!(
                "Add --import-jsonl to preserve these records in the destination before switching."
            );
        }
        println!(
            "No files changed or database connection attempted. Destination conflicts/permissions are checked on apply."
        );
        println!(
            "Stop all source writers and supervisors, then repeat with --apply --confirm-stopped."
        );
        return Ok(());
    }
    if !records.is_empty() && !options.import_jsonl {
        bail!("JSONL contains records; repeat with --import-jsonl. Configuration was not changed");
    }
    crate::ensure_local_ignore(&paths.directory.join(".gitignore"))?;
    let backup = if changed {
        Some(edit.backup(paths)?)
    } else {
        None
    };
    if let Some(path) = &backup {
        println!("Config backup: {}", path.display());
    }
    // Initialize/check first, import second, switch config last. A failed import
    // rolls itself back; initialized schemas/projects can remain on failure.
    let destination_result: Result<_> = async {
        let storage = Storage::open(&edit.next, paths, true).await?;
        storage.check().await?;
        edit.ensure_unchanged()?;
        ensure_source_unchanged(paths, &source, edit.previous.storage.is_jsonl())?;
        let (imported, skipped) = if options.import_jsonl {
            storage.import_records(&records, true).await?
        } else {
            (0, 0)
        };
        Ok((storage, imported, skipped))
    }
    .await;
    let (storage, imported, skipped) = destination_result.context(
        "setup did not switch configuration; destination schema/project may remain. Source JSONL is unchanged by setup",
    )?;
    ensure_source_unchanged(paths, &source, edit.previous.storage.is_jsonl())
        .context("configuration not switched; imported destination records may remain, so reconcile before retrying")?;
    if changed {
        edit.commit().context("could not finish configuration switch; keep the config backup and source JSONL, inspect config before retrying; destination data may remain")?;
    } else {
        edit.ensure_unchanged()?;
    }
    println!(
        "Storage ready ({}): {imported} imported, {skipped} identical records skipped. Source JSONL retained; no ongoing sync.",
        storage.name()
    );
    println!("Run `jevia storage check` and `jevia stats` to verify. No Jev API request was made.");
    Ok(())
}

fn read_optional(path: &Path) -> Result<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).context("could not read source JSONL history"),
    }
}

fn ensure_source_unchanged(
    paths: &ProjectPaths,
    original: &Option<Vec<u8>>,
    check: bool,
) -> Result<()> {
    if check && &read_optional(&paths.runs)? != original {
        bail!("source JSONL changed during setup; stop all writers before retrying");
    }
    Ok(())
}

fn config_lock(paths: &ProjectPaths) -> Result<fs::File> {
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
            bail!("another storage setup is running; retry after it finishes")
        }
        Err(fs::TryLockError::Error(error)) => {
            Err(error).context("could not lock configuration for setup")
        }
    }
}

fn protect_sqlite_target(paths: &ProjectPaths, storage: &StorageConfig) -> Result<()> {
    let StorageConfig::Sqlite { url } = storage else {
        return Ok(());
    };
    let target = paths.root.join(
        url.strip_prefix("sqlite://")
            .context("invalid SQLite file URL")?,
    );
    if let Ok(metadata) = fs::symlink_metadata(&target) {
        if !metadata.is_file() {
            bail!("SQLite setup target must be a regular file, not a symlink or directory");
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if metadata.nlink() > 1 {
                bail!("SQLite setup refuses hard-linked destination files");
            }
        }
        let canonical = fs::canonicalize(&target)?;
        for protected in [
            &paths.config,
            &paths.runs,
            &paths.cache,
            &paths.directory.join(".gitignore"),
            &paths.directory.join("config.lock"),
            &paths.directory.join("runs.lock"),
            &paths.directory.join("cache.lock"),
        ] {
            if fs::canonicalize(protected).is_ok_and(|p| p == canonical) {
                bail!("SQLite target overlaps a Jevia configuration, history, cache, or lock file");
            }
        }
    }
    // Reserve these basenames everywhere, including before files exist and on
    // case-insensitive filesystems. A missing RUNS.JSONL must not become a database.
    if target
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            [
                "config.toml",
                "runs.jsonl",
                "cache.jsonl",
                ".gitignore",
                "config.lock",
                "runs.lock",
                "cache.lock",
            ]
            .iter()
            .any(|reserved| name.eq_ignore_ascii_case(reserved))
        })
    {
        bail!("SQLite target uses a reserved Jevia file name");
    }
    Ok(())
}

struct ConfigEdit {
    path: PathBuf,
    original: String,
    rendered: String,
    permissions: fs::Permissions,
    previous: Config,
    next: Config,
}

impl ConfigEdit {
    fn prepare(path: &Path, storage: StorageConfig) -> Result<Self> {
        let metadata = fs::symlink_metadata(path)?;
        if !metadata.is_file() {
            bail!("setup requires a regular config.toml, not a symlink");
        }
        let original = fs::read_to_string(path)?;
        let previous = Config::from_toml(&original).map_err(|_| {
            anyhow!("invalid project configuration; setup refused (contents redacted)")
        })?;
        let mut next = previous.clone();
        next.storage = storage;
        next.validate()?;
        let rendered = if previous.storage == next.storage {
            original.clone()
        } else {
            let mut document = original
                .parse::<DocumentMut>()
                .map_err(|_| anyhow!("could not edit project configuration (contents redacted)"))?;
            document["storage"] = storage_item(&next.storage);
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

    fn ensure_unchanged(&self) -> Result<()> {
        if !fs::symlink_metadata(&self.path)?.is_file()
            || fs::read_to_string(&self.path)? != self.original
        {
            bail!("configuration changed during setup; refusing to overwrite concurrent edits");
        }
        Ok(())
    }

    fn backup(&self, paths: &ProjectPaths) -> Result<PathBuf> {
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

    fn commit(&self) -> Result<()> {
        let directory = self
            .path
            .parent()
            .context("configuration path has no parent")?;
        let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
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

fn storage_item(storage: &StorageConfig) -> Item {
    let mut table = Table::new();
    match storage {
        StorageConfig::Sqlite { url } => {
            table["backend"] = value("sqlite");
            table["url"] = value(url);
        }
        StorageConfig::Postgres {
            url_env,
            project,
            allow_insecure_localhost,
        } => {
            table["backend"] = value("postgres");
            table["url_env"] = value(url_env);
            table["project"] = value(project);
            if *allow_insecure_localhost {
                table["allow_insecure_localhost"] = value(true);
            }
        }
        StorageConfig::Jsonl => unreachable!("setup targets databases only"),
    }
    Item::Table(table)
}

fn sync_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    fs::File::open(path)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths() -> (tempfile::TempDir, ProjectPaths) {
        let directory = tempfile::tempdir().unwrap();
        let paths = ProjectPaths::at(directory.path().to_owned());
        fs::create_dir(&paths.directory).unwrap();
        fs::write(&paths.config, Config::default().to_toml().unwrap()).unwrap();
        (directory, paths)
    }

    #[test]
    fn concurrent_config_edits_and_source_changes_are_not_overwritten() {
        let (_dir, paths) = paths();
        let edit = ConfigEdit::prepare(
            &paths.config,
            StorageConfig::Sqlite {
                url: "sqlite://.jevia/test.db".into(),
            },
        )
        .unwrap();
        let edited = format!("# another editor\n{}", edit.original);
        fs::write(&paths.config, &edited).unwrap();
        assert!(edit.commit().is_err());
        assert_eq!(fs::read_to_string(&paths.config).unwrap(), edited);
        assert!(ensure_source_unchanged(&paths, &None, true).is_ok());
        fs::write(&paths.runs, b"changed").unwrap();
        assert!(ensure_source_unchanged(&paths, &None, true).is_err());
        assert!(ensure_source_unchanged(&paths, &Some(b"original".to_vec()), true).is_err());
    }

    #[test]
    fn setup_lock_excludes_other_setups_and_releases_on_drop() {
        let (_dir, paths) = paths();
        let lock = config_lock(&paths).unwrap();
        assert!(config_lock(&paths).is_err());
        drop(lock);
        assert!(config_lock(&paths).is_ok());
    }

    #[test]
    fn storage_table_inline_and_dotted_forms_preserve_other_settings() {
        for storage in ["", "[storage]\nbackend = 'jsonl'\n"] {
            let (_dir, paths) = paths();
            let original = format!(
                "# policy note\n{}\n{storage}",
                Config::default().to_toml().unwrap()
            );
            fs::write(&paths.config, &original).unwrap();
            let edit = ConfigEdit::prepare(
                &paths.config,
                StorageConfig::Sqlite {
                    url: "sqlite://.jevia/test.db".into(),
                },
            )
            .unwrap();
            assert!(edit.rendered.contains("# policy note"));
            let backup = edit.backup(&paths).unwrap();
            assert_eq!(fs::read_to_string(&backup).unwrap(), original);
            edit.commit().unwrap();
            let repeated = ConfigEdit::prepare(&paths.config, edit.next.storage.clone()).unwrap();
            assert_eq!(repeated.rendered, edit.rendered);
        }
        for storage in [
            "storage = { backend = 'jsonl' }",
            "storage.backend = 'jsonl'",
        ] {
            let (_dir, paths) = paths();
            fs::write(
                &paths.config,
                format!("{storage}\n{}", Config::default().to_toml().unwrap()),
            )
            .unwrap();
            let edit = ConfigEdit::prepare(
                &paths.config,
                StorageConfig::Postgres {
                    url_env: "DATABASE_URL".into(),
                    project: "test".into(),
                    allow_insecure_localhost: false,
                },
            )
            .unwrap();
            assert_eq!(Config::from_toml(&edit.rendered).unwrap(), edit.next);
        }
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_hard_links_and_private_backups() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let (_dir, paths) = paths();
        let edit = ConfigEdit::prepare(
            &paths.config,
            StorageConfig::Sqlite {
                url: "sqlite://.jevia/test.db".into(),
            },
        )
        .unwrap();
        let backup = edit.backup(&paths).unwrap();
        assert_eq!(
            fs::metadata(&backup).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(backup.parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        let linked = paths.directory.join("linked.db");
        symlink(&paths.config, &linked).unwrap();
        assert!(
            protect_sqlite_target(
                &paths,
                &StorageConfig::Sqlite {
                    url: "sqlite://.jevia/linked.db".into()
                }
            )
            .is_err()
        );
        fs::write(&paths.runs, "").unwrap();
        fs::hard_link(&paths.runs, paths.directory.join("hard.db")).unwrap();
        assert!(
            protect_sqlite_target(
                &paths,
                &StorageConfig::Sqlite {
                    url: "sqlite://.jevia/hard.db".into()
                }
            )
            .is_err()
        );
        fs::rename(&paths.config, paths.directory.join("original.toml")).unwrap();
        symlink(paths.directory.join("original.toml"), &paths.config).unwrap();
        assert!(edit.commit().is_err());
        assert!(ConfigEdit::prepare(&paths.config, edit.next.storage).is_err());
    }
}
