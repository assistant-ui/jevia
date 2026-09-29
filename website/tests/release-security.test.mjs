import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const workflow = readFileSync(
  new URL("../../.github/workflows/release-binaries.yml", import.meta.url),
  "utf8",
);

test("release binaries receive provenance and SBOM attestations", () => {
  assert.match(workflow, /artifact-metadata: write/);
  assert.match(workflow, /attestations: write/);
  assert.match(workflow, /id-token: write/);
  assert.match(workflow, /uses: anchore\/sbom-action@v0/);
  assert.match(workflow, /format: spdx-json/);
  assert.equal(workflow.match(/uses: actions\/attest@v4/g)?.length, 2);
  assert.match(workflow, /sbom-path:/);
});

test("release smoke tests verify provenance and SPDX attestations", () => {
  const smokeJob = workflow.slice(
    workflow.indexOf("  smoke:"),
    workflow.indexOf("  sync-installer:"),
  );

  assert.doesNotMatch(smokeJob, /ref: \$\{\{ env\.RELEASE_TAG \}\}/);
  assert.match(workflow, /gh attestation verify "\$install_dir\/jevia"/);
  assert.match(workflow, /gh attestation verify \$binary/);
  assert.equal(
    workflow.match(/--predicate-type https:\/\/spdx\.dev\/Document\/v2\.3/g)?.length,
    2,
  );
});
