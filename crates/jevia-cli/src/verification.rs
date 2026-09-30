//! Conservative, read-only discovery. Never install dependencies or execute a probe.
use std::{
    fs::{self, OpenOptions},
    io::Read,
    path::Path,
};

use jevia_core::VerificationInvocation;
use serde_json::Value;

pub enum Detection {
    Found(VerificationInvocation),
    Unavailable(&'static str),
}

const MAX_MANIFEST_BYTES: u64 = 256 * 1024;

fn manifest(path: &Path) -> Result<Option<String>, &'static str> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() => {}
        Ok(_) => {
            return Err(
                "Automatic verification requires a regular project manifest; configure an explicit verifier.",
            );
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("Project manifest is unreadable; configure an explicit verifier."),
    }
    let mut options = OpenOptions::new();
    options.read(true);
    // Do not block on a FIFO swapped in between metadata inspection and open.
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(nix::libc::O_NONBLOCK);
    }
    let file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("Project manifest is unreadable; configure an explicit verifier."),
    };
    if !file.metadata().is_ok_and(|metadata| metadata.is_file()) {
        return Err(
            "Automatic verification requires a regular project manifest; configure an explicit verifier.",
        );
    }
    let mut raw = String::new();
    file.take(MAX_MANIFEST_BYTES + 1)
        .read_to_string(&mut raw)
        .map_err(|_| "Project manifest could not be read; configure an explicit verifier.")?;
    if raw.len() as u64 > MAX_MANIFEST_BYTES {
        return Err(
            "Project manifest is too large for automatic detection; configure an explicit verifier.",
        );
    }
    Ok(Some(raw))
}

pub fn detect(root: &Path) -> Detection {
    match discover(root) {
        Ok(Some(plan)) => Detection::Found(plan),
        Ok(None) => Detection::Unavailable(
            "No usable root test command found. Configure a verifier once; process exits alone are not learning evidence.",
        ),
        Err(message) => Detection::Unavailable(message),
    }
}

fn discover(root: &Path) -> Result<Option<VerificationInvocation>, &'static str> {
    let mut candidates = Vec::new();
    if let Some(raw) = manifest(&root.join("Cargo.toml"))? {
        let cargo: toml_edit::DocumentMut = raw
            .parse()
            .map_err(|_| "Cargo manifest is invalid; automatic verification is unavailable.")?;
        if cargo.get("package").is_some() || cargo.get("workspace").is_some() {
            candidates.push(VerificationInvocation {
                program: "cargo".into(),
                args: vec!["test".into(), "--workspace".into()],
            });
        }
    }
    if let Some(raw) = manifest(&root.join("package.json"))? {
        let package: Value = serde_json::from_str(&raw)
            .map_err(|_| "Node manifest is invalid; automatic verification is unavailable.")?;
        if let Some(test) = package
            .get("scripts")
            .and_then(|scripts| scripts.get("test"))
            .and_then(Value::as_str)
        {
            let test = test.trim();
            if !test.is_empty()
                && !test.contains("no test specified")
                && !matches!(test, "true" | "exit 0" | ":")
            {
                let manager = manager(&package, root)?;
                let program = if cfg!(windows) && manager != "bun" {
                    format!("{manager}.cmd")
                } else {
                    manager.into()
                };
                candidates.push(VerificationInvocation {
                    program,
                    args: vec!["run".into(), "test".into()],
                });
            }
        }
    }
    if candidates.len() > 1 {
        return Err(
            "Both Rust and Node tests are present; configure a verifier covering the intended checks instead of guessing.",
        );
    }
    Ok(candidates.pop())
}

fn manager(package: &Value, root: &Path) -> Result<&'static str, &'static str> {
    if let Some(declared) = package.get("packageManager") {
        return match declared.as_str().and_then(|value| value.split('@').next()) {
            Some("npm") => Ok("npm"),
            Some("pnpm") => Ok("pnpm"),
            Some("yarn") => Ok("yarn"),
            Some("bun") => Ok("bun"),
            _ => Err("Unsupported package manager declaration; configure an explicit verifier."),
        };
    }
    let managers: Vec<_> = [
        ("npm", ["package-lock.json", "npm-shrinkwrap.json"]),
        ("pnpm", ["pnpm-lock.yaml", "pnpm-lock.yaml"]),
        ("yarn", ["yarn.lock", "yarn.lock"]),
        ("bun", ["bun.lock", "bun.lockb"]),
    ]
    .into_iter()
    .filter_map(|(name, locks)| {
        locks
            .iter()
            .any(|file| root.join(file).is_file())
            .then_some(name)
    })
    .collect();
    match managers.as_slice() {
        [] => Ok("npm"),
        [manager] => Ok(manager),
        _ => Err(
            "Conflicting package-manager lockfiles; configure packageManager or an explicit verifier.",
        ),
    }
}

#[cfg(test)]
mod tests;
