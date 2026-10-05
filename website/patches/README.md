# Website dependency patches

## Farm 0.1.0 globbing

`@farm.js__core@0.1.0.patch` switches the three async glob call sites
(and their emitted ESM/CommonJS copies) to `tinyglobby.glob`. The package extension
adds pinned `tinyglobby@0.2.17`; the scoped override removes Farm's `fast-glob`
dependency, eliminating its `micromatch` → `braces` chain.

This mitigates [GHSA-vfj7-8cjw-p6xm](https://github.com/advisories/GHSA-vfj7-8cjw-p6xm),
which currently has no patched `braces` release. No advisory is ignored and
`pnpm audit` remains an unfiltered CI gate. The patch is version-scoped so a Farm
upgrade requires review. Remove the patch, override, and package extension together
when Farm ships an equivalent migration or a safe upstream dependency path.

The patch was regenerated against the published stable `0.1.0` package. Its three
source call sites are emitted in three ESM chunks and five CommonJS bundles
(18 replacements in total); the stable release still imports `fast-glob` upstream.

`tests/farm-glob.test.mjs` checks this installed bundle inventory and exercises
the actual patched ESM/CommonJS entry points, route alternatives, nested files,
hidden-directory exclusion, and empty matches.
The production build and `test:build` cover discovery of the website and API routes.

Keep `@farm.js/cli` and `@farm.js/core` on the same exact version. Dependabot groups
them, but each upgrade still needs a reviewed patch migration: update the scoped
Scalar override, removed `fast-glob` edge, `tinyglobby` extension, and patch key
together. Regenerate the patch against the new published bundles, not just the
old filenames. The upgrade contract test checks that these keys target the
installed version; `pnpm audit` catches transitive advisories. Never remove the
security gates or ignore advisories to land a version update.

## Retired Sharp package-export patch

Farm 0.1.0 resolves package metadata from the package entry point when the manifest
is not exported, so the previous `sharp@0.35.4.patch` is no longer needed. Farm now
uses Sharp 0.35.5 without patching its exports. The existing security override
still prevents other dependency paths from resolving Sharp below 0.35.4.

`tests/build/runtime.test.mjs` verifies the bundled version through Sharp's public
`versions.sharp` property, encodes a PNG, and reads its metadata using the packaged
native runtime. Do not restore a private manifest export just to check a version.
