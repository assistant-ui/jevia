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
still prevents other dependency paths from resolving Sharp below 0.35.5.

`tests/build/runtime.test.mjs` verifies the bundled version through Sharp's public
`versions.sharp` property, encodes a PNG, and reads its metadata using the packaged
native runtime. Do not restore a private manifest export just to check a version.

## Gray-matter YAML engine

The version-scoped `gray-matter@4.0.3.patch` migrates its two YAML API calls from
`safeLoad`/`safeDump` to JS-YAML 4's `load`/`dump`. The accompanying dependency
override selects JS-YAML 4.3.2, whose default schema excludes JavaScript-specific
tags. This removes the JS-YAML 3 → argparse 1 → sprintf-js dependency path for
[GHSA-hp3w-g68c-fv3c](https://github.com/advisories/GHSA-hp3w-g68c-fv3c), which has
no patched sprintf-js release. An override alone is not sufficient: the old
method names throw in JS-YAML 4.

`tests/frontmatter.test.mjs` resolves the parser through Farm's installed docs
dependency and checks metadata, body content, YAML round-trips, invalid input,
and rejection of JavaScript-specific YAML tags. Remove the patch and override
together when gray-matter adopts a safe dependency path upstream.

The source-map-js 1.2.2 and Sharp 0.35.5 minimum-version overrides also cover
current audit findings. The scoped Geist override removes its unused Next.js
peer: Jevia loads only the bundled font files through Farm's `localFont`, never
Geist's Next.js font adapters. This avoids installing a second web framework and
its advisory-bearing dependency tree. Audit remains unfiltered.
