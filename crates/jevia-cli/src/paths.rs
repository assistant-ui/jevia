use std::{env, path::PathBuf};

use anyhow::{Context, Result, bail};

#[derive(Debug, Clone)]
pub struct ProjectPaths {
    pub root: PathBuf,
    pub directory: PathBuf,
    pub config: PathBuf,
    pub runs: PathBuf,
    pub cache: PathBuf,
}

impl ProjectPaths {
    pub fn at(root: PathBuf) -> Self {
        let directory = root.join(".jevia");
        Self {
            root,
            config: directory.join("config.toml"),
            runs: directory.join("runs.jsonl"),
            cache: directory.join("cache.jsonl"),
            directory,
        }
    }

    pub fn current() -> Result<Self> {
        let current = env::current_dir().context("could not read the current directory")?;
        Ok(Self::at(current))
    }

    pub fn discover() -> Result<Self> {
        let current = env::current_dir().context("could not read the current directory")?;
        for ancestor in current.ancestors() {
            let paths = Self::at(ancestor.to_path_buf());
            if paths.config.is_file() {
                return Ok(paths);
            }
        }

        bail!("no .jevia/config.toml found; run jevia init from the project root")
    }
}
