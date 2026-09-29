import { readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const VERSION_PATTERN = /^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/;
const INSTALLER_VERSION_PATTERN = /JEVIA_VERSION="\$\{JEVIA_VERSION:-([^}]+)\}"/;
const GUIDE_RELEASE_PATTERN =
  /downloads the matching Jevia [0-9A-Za-z.+-]+ binary from https:\/\/github\.com\/assistant-ui\/jevia\/releases\/tag\/v[0-9A-Za-z.+-]+,/;

function replaceExactlyOnce(source, pattern, replacement, label) {
  const matches = source.match(new RegExp(pattern.source, "g")) ?? [];
  if (matches.length !== 1) {
    throw new Error(`expected exactly one ${label}, found ${matches.length}`);
  }
  return source.replace(pattern, replacement);
}

export function syncInstallerSources(installerSource, guideSource, version) {
  if (!VERSION_PATTERN.test(version)) {
    throw new Error(`invalid release version: ${version}`);
  }

  const installer = replaceExactlyOnce(
    installerSource,
    INSTALLER_VERSION_PATTERN,
    `JEVIA_VERSION="\${JEVIA_VERSION:-${version}}"`,
    "installer version declaration",
  );
  const guide = replaceExactlyOnce(
    guideSource,
    GUIDE_RELEASE_PATTERN,
    `downloads the matching Jevia ${version} binary from https://github.com/assistant-ui/jevia/releases/tag/v${version},`,
    "install-guide release reference",
  );

  return { installer, guide };
}

function run() {
  const version = process.argv[2]?.replace(/^v/, "");
  if (!version) {
    throw new Error("usage: node scripts/sync-installer-version.mjs <version>");
  }

  const repositoryRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
  const installerPath = resolve(repositoryRoot, "website/public/install.sh");
  const guidePath = resolve(repositoryRoot, "website/src/lib/install-guide.ts");
  const currentInstaller = readFileSync(installerPath, "utf8");
  const currentGuide = readFileSync(guidePath, "utf8");
  const next = syncInstallerSources(currentInstaller, currentGuide, version);

  if (next.installer !== currentInstaller) {
    writeFileSync(installerPath, next.installer);
  }
  if (next.guide !== currentGuide) {
    writeFileSync(guidePath, next.guide);
  }
}

const invokedPath = process.argv[1] ? pathToFileURL(resolve(process.argv[1])).href : "";
if (invokedPath === import.meta.url) {
  run();
}
