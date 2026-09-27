# Releasing Jevia

The workspace publishes two crates: `jevia-core` and the `jevia` CLI. Keep the
workspace version and the CLI's `jevia-core` dependency version in sync.

## Prepare

1. Start a `chore/release-<version>` branch from current `main`.
2. Update the version in the root `Cargo.toml`, the `jevia-core` dependency in
   `crates/jevia-cli/Cargo.toml`, and the lockfile. Update the pinned Cargo
   installation documentation and changelog entry. Keep the website installer
   pinned to the working release until the new binary assets are available.
3. Confirm both packaged archives contain their README and MIT license, and no
   credentials or local `.jevia` data:

   ```bash
   cargo package --list -p jevia-core
   cargo package --list -p jevia
   ```

4. Commit the preparation and run the release checks:

   ```bash
   cargo fmt --all -- --check
   cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
   cargo test --workspace --all-features --locked
   cargo publish --workspace --dry-run --locked --registry crates-io
   ```

5. Open a pull request and wait for all CI checks, including macOS and Windows,
   before merging it. Publish from the clean merged commit, not a PR checkout.

## Publish

Authenticate with `cargo login` using a crates.io token authorized to publish
both crate names. Keep tokens out of commits, command arguments, and logs.
First-time publishers also need a verified email address on crates.io.

```bash
cargo publish --workspace --locked --registry crates-io
```

Cargo 1.92 publishes the dependency first and waits for registry availability.
If the upload only partially succeeds, check crates.io and publish only the
missing package with `cargo publish -p <package> --locked --registry crates-io`.
Never assume an upload failed just because waiting for the index timed out.
Published version contents cannot be overwritten.

## Verify and tag

Install the exact published CLI version into an isolated temporary directory
with `cargo install jevia --version <version> --locked --root <directory>`.
Check `jevia --version`, `jevia --help`, `jevia init`, `jevia runs --json`, and
`jevia cache status` in a temporary project. `jevia doctor` should report a
missing API key when none is configured; it should not contact Jev.

Only after registry installation succeeds, create an annotated `v<version>`
tag on the published commit and push it. The `Release binaries` workflow builds
Intel and ARM binaries for macOS and Linux, creates or updates the GitHub
release, and uploads each binary with its SHA-256 checksum. Do not announce the
installer until all release assets are present, and do not mark a GitHub release
as published to crates.io if either crate is missing.

After all four binaries and their checksums are uploaded and verified, update
`JEVIA_VERSION` in `website/public/install.sh`, the versioned website install
guide, and installer test fixtures in a follow-up PR. Run the website tests and
wait for CI before merging. This ordering avoids pointing the deployed installer
at assets that do not exist yet. Smoke-test the installer in an isolated directory.

For an existing tag whose release assets need to be rebuilt, run the `Release
binaries` workflow manually and supply the exact tag, such as `v0.1.0`.
