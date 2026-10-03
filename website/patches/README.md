# Website dependency patches

## Farm 0.1.0-beta.107 globbing

`@farm.js__core@0.1.0-beta.107.patch` switches the three async glob call sites
(and their emitted ESM/CommonJS copies) to `tinyglobby.glob`. The package extension
adds pinned `tinyglobby@0.2.17`; the scoped override removes Farm's `fast-glob`
dependency, eliminating its `micromatch` → `braces` chain.

This mitigates [GHSA-vfj7-8cjw-p6xm](https://github.com/advisories/GHSA-vfj7-8cjw-p6xm),
which currently has no patched `braces` release. No advisory is ignored and
`pnpm audit` remains an unfiltered CI gate. The patch is version-scoped so a Farm
upgrade requires review. Remove the patch, override, and package extension together
when Farm ships an equivalent migration or a safe upstream dependency path.

`tests/farm-glob.test.mjs` exercises the actual patched ESM/CommonJS entry points,
route alternatives, nested files, hidden-directory exclusion, and empty matches.
The production build and `test:build` cover discovery of the website and API routes.

## Sharp 0.35.4

`sharp@0.35.4.patch` exposes `sharp/package.json` for Farm's version lookup.
