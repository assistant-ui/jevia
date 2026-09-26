# Website dependency security

## Verification

Use the pinned pnpm version from `package.json` and Node 22.13+:

```sh
pnpm install --frozen-lockfile
pnpm audit
pnpm test
pnpm type-check
pnpm build
pnpm test:build
```

CI runs all of these checks. No audit advisories are ignored and audit failures
are not suppressed. Also review GitHub Dependabot: the npm audit response does
not necessarily include every GitHub advisory, especially prerelease packages.
A clean audit is not proof that all GitHub alerts have been resolved.

## Temporary transitive overrides

Farm core and CLI are pinned to `0.1.0-beta.104`. Their dependency constraints
still resolve vulnerable packages. The root overrides are restricted to the
affected version lines, leaving H3 1.x and the bundled Vite 8 build unchanged.

| Package | Selected version | Reason |
| --- | --- | --- |
| `@ai-sdk/provider-utils` 4.x | 4.0.33 | Fix GHSA-866g-f22w-33x8 without moving to AI SDK 7 dependencies |
| `h3` 2.0.1 prereleases | 2.0.1-rc.18 | Fix the six H3 advisories through GHSA-q5pr-72pq-83v3 |
| `sharp` | 0.35.4 | Fix GHSA-f88m-g3jw-g9cj and GHSA-rgj7-g3m4-5g8c |
| `srvx` | 0.11.13 | Fix GHSA-p36q-q72m-gchr |
| `uuid` | 11.1.1 | Fix GHSA-w5hq-g745-h8pq while keeping CommonJS support for Sequelize |
| `vite` | 7.3.6 | Fix the Vite alerts and remove vulnerable esbuild 0.21; satisfy Nitro's Vite 7 peer |
| `yaml` 2.x | 2.8.3 | Fix GHSA-48c2-rrv3-qjmp |

Review these overrides when upgrading Farm; remove them only when the upstream
dependency graph selects safe versions without them. Keep the frozen lockfile
and run the runtime tests on dependency changes.

## Compatibility patches

Two small patches allow the security upgrades without changing the framework or
website UI:

- `sharp@0.35.4.patch` exports `sharp/package.json` again. Farm's Vercel packaging
  code reads it to copy Sharp and its native dependencies. The patch changes
  no image-processing code. The build test performs a PNG encode/decode using
  Sharp from the final serverless function, not the development dependency tree.
- `nitro@3.0.1-alpha.0.patch` updates three H3 internal method names (`~request`,
  `~findRoute`, `~getMiddleware`) to the pinned patched H3 release. It retains
  request context and middleware behavior. The runtime tests check SSR and 404
  responses; the prerender test catches builds that silently emit no homepage.

The Nitro patch is **compatibility only**, not a fix for the two advisories below.
Remove both patches when Farm supports patched dependencies natively.

## Remaining Nitro alerts — not dismissed

As of this cleanup, `nitro@3.0.1-alpha.0` still falls within two moderate GitHub
advisory ranges:

- [GHSA-9phm-9p8f-hw5m](https://github.com/nitrojs/nitro/security/advisories/GHSA-9phm-9p8f-hw5m): wildcard route-rule open redirect.
- [GHSA-5w89-w975-hf9q](https://github.com/nitrojs/nitro/security/advisories/GHSA-5w89-w975-hf9q): wildcard route-rule proxy scope bypass.

Both are fixed in `nitro@3.0.260429-beta`. That release moved builder exports
from `nitro` to `nitro/builder` and removed `nitro/runtime`, which Farm currently
imports and generates in its server bundle. It is not a drop-in override.

This site currently configures only the Vercel deployment target, has one static
page, and defines no wildcard redirect or proxy route rules. The advisories'
documented route-rule prerequisites are therefore absent in this configuration.
This is an exposure assessment, **not** a claim that Nitro is patched.

Do not add redirect/proxy route rules until Farm supports a fixed Nitro release
or a separately reviewed backport is tested. Keep these GitHub alerts open and
track the upstream-compatible Nitro upgrade as the remaining security follow-up.

The updated lockfile is intended to address 16 of the original 18 alerts,
including all five high-severity alerts. Confirm GitHub's rescan after merging;
do not infer alert closure from a local `pnpm audit` result.
