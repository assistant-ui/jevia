import type { Metadata } from "@farm.js/core";
import { ArrowUpRight } from "lucide-react";

import { CodeBlock } from "../../components/code-block";
import { CommandBlock } from "../../components/command-block";
import { DocsSidebar } from "../../components/docs-sidebar";
import { SiteHeader } from "../../components/site-header";

export const dynamic = "force-static";
export const metadata: Metadata = {
  title: "Documentation — Jevia",
  description:
    "Install Jevia, configure adaptive outcome-based routing, connect any coding harness, and use the typed Node.js SDK.",
  alternates: { canonical: "/docs" },
  openGraph: {
    title: "Documentation — Jevia",
    description:
      "A practical guide to Jevia routing, verified outcomes, harness adapters, and the Node.js SDK.",
    url: "/docs",
    type: "website",
  },
};

const CLI_SETUP = [
  "curl -fsSL https://jevia.dev/install.sh | sh",
  "jevia --version",
  "jevia init",
  'export TYPESAFE_API_KEY="your-key"',
  "jevia check",
  'jevia route "fix the flaky integration test"',
].join("\n");

const CLI_WORKFLOW = [
  'jevia route "investigate the failing integration test"',
  'jevia route --json "investigate the failing integration test"',
  'jevia run agent "investigate the failing integration test"',
  "jevia runs",
  "jevia runs show <run-id>",
  "jevia stats --json",
].join("\n");

const CLI_COMMANDS = [
  {
    signature: "route <task>",
    description: "Select a capability tier and save the routing decision as a new run.",
  },
  {
    signature: "run <harness> <task>",
    description: "Route the task, launch a configured harness, verify it, and record the result.",
  },
  {
    signature: "runs [--json]",
    description: "List recent run records, their lifecycle state, outcome source, and learning eligibility.",
  },
  {
    signature: "runs show <run-id>",
    description: "Inspect one complete versioned record, including execution and outcome evidence.",
  },
  {
    signature: "stats [--limit N] [--json]",
    description: "Summarize routing decisions, trusted outcomes, manual feedback, and cache hits.",
  },
] as const;

const ADAPTIVE_LOOP = [
  {
    signature: "01  Route",
    description: "Jevia selects one configured capability tier for the current task.",
  },
  {
    signature: "02  Execute",
    description: "Your chosen harness maps that tier to a concrete model and performs the work.",
  },
  {
    signature: "03  Verify",
    description: "Tests, review, or another trusted evaluator decides whether the task succeeded.",
  },
  {
    signature: "04  Record",
    description: "The verified outcome is attached to the run with its evidence source.",
  },
  {
    signature: "05  Adapt",
    description: "Eligible outcomes become evidence for later routing decisions.",
  },
] as const;

const SDK_EXAMPLE = [
  'import { JeviaClient } from "jevia";',
  "",
  "const jevia = new JeviaClient({ cwd: process.cwd() });",
  'const task = "fix the flaky integration test";',
  "const route = await jevia.route(task);",
  "",
  "const result = await runYourHarness({",
  "  task,",
  "  tier: route.tier,",
  "  runId: route.run_id,",
  "});",
  "",
  "const passed = await verifyResult(result);",
  "await jevia.feedback(",
  "  route.run_id,",
  '  passed ? "success" : "failure",',
  ");",
].join("\n");

const SDK_STORAGE_SETUP = [
  'const target = { backend: "sqlite", path: ".jevia/jevia.db" } as const;',
  "",
  "// Preview only: no database connection or file changes.",
  "console.log(await jevia.setupStorage(target, { importJsonl: true }));",
  "",
  "// Stop project writers and supervisors before applying.",
  "await jevia.setupStorage(target, {",
  "  apply: true,",
  "  confirmStopped: true,",
  "  importJsonl: true,",
  "});",
  "console.log(await jevia.checkStorage());",
  "console.log(await jevia.checkStorage({ deep: true }));",
].join("\n");

const SDK_POSTGRES_SETUP = [
  "// Set JEVIA_DATABASE_URL before creating the client.",
  "const target = {",
  '  backend: "postgres",',
  '  project: "my-app",',
  '  urlEnv: "JEVIA_DATABASE_URL",',
  "} as const;",
  "",
  "console.log(await jevia.setupStorage(target)); // Preview first.",
].join("\n");

const SDK_ERROR_HANDLING = [
  'import { JeviaClient, JeviaCommandError } from "jevia";',
  "",
  "try {",
  "  await jevia.route(task, { signal: controller.signal });",
  "} catch (error) {",
  "  if (error instanceof JeviaCommandError) {",
  "    console.error(error.message, error.exitCode, error.signal);",
  "  }",
  "}",
].join("\n");

const METHODS = [
  {
    signature: "version(options?)",
    description: "Return the installed Jevia CLI version after validating its output.",
  },
  {
    signature: "route(task, options?)",
    description: "Choose a capability tier and return the complete typed route record.",
  },
  {
    signature: "feedback(runId, outcome, options?)",
    description: "Record an explicit success, failure, or unknown outcome for a run.",
  },
  {
    signature: "runs(options?)",
    description: "List recent route records with a configurable positive result limit.",
  },
  {
    signature: "show(runId, options?)",
    description: "Read one complete record, including lifecycle and outcome evidence.",
  },
  {
    signature: "setupStorage(target, options?)",
    description: "Unreleased: preview SQLite or PostgreSQL setup and apply it only with confirmation.",
  },
  {
    signature: "checkStorage(options?)",
    description: "Unreleased: check selected storage or deeply validate it without a write probe.",
  },
] as const;

const HARNESS_SETUP = [
  "jevia harness setup agent --command my-agent \\",
  "  --arg=run --arg=--model --arg='{model}' --arg='{task}' \\",
  "  --model fast=provider/small \\",
  "  --model balanced=provider/standard \\",
  "  --model strong=provider/frontier \\",
  "  --verify-command cargo --verify-arg=test",
].join("\n");

const HARNESS_COMMANDS = [
  "# Review the generated TOML, then save it",
  "jevia harness setup agent --command my-agent \\",
  "  --arg=run --arg=--model --arg='{model}' --arg='{task}' \\",
  "  --model fast=provider/small \\",
  "  --model balanced=provider/standard \\",
  "  --model strong=provider/frontier \\",
  "  --verify-command cargo --verify-arg=test --apply",
  "",
  "# Validate templates and local executable paths without launching anything",
  "jevia harness check agent",
  "jevia harness check agent --json",
  "",
  "# Route and launch the configured harness",
  'jevia run agent "investigate the failing integration test"',
].join("\n");

const OUTCOME_COMMANDS = [
  "jevia feedback <run-id> success",
  'jevia feedback <run-id> failure --reason "The integration test still fails"',
  'jevia feedback <run-id> unknown --reason "No reliable verification result"',
  "jevia runs show <run-id>",
].join("\n");

const EVIDENCE_TYPES = [
  {
    signature: "verification",
    description: "A configured verifier completed and produced a known result. Eligible for learning.",
  },
  {
    signature: "manual",
    description: "A person explicitly recorded success or failure. Eligible for learning.",
  },
  {
    signature: "process_exit",
    description: "The harness process exited. Visible for diagnosis, but not learning evidence by itself.",
  },
  {
    signature: "unknown",
    description: "No trusted result is known. It stays out of the adaptive evidence set.",
  },
] as const;

const SQLITE_SETUP = [
  "# Preview the migration; this writes nothing",
  "jevia storage setup sqlite --import-jsonl",
  "",
  "# Stop Jevia writers and supervisors, then apply the reviewed plan",
  "jevia storage setup sqlite --import-jsonl --apply --confirm-stopped",
  "jevia storage check",
  "jevia stats",
].join("\n");

const POSTGRES_SETUP = [
  'export JEVIA_DATABASE_URL="postgresql://..."',
  "",
  "# Preview first; the URL stays in the environment",
  "jevia storage setup postgres --project my-project --import-jsonl",
  "",
  "# Stop writers and supervisors before applying",
  "jevia storage setup postgres --project my-project --import-jsonl --apply --confirm-stopped",
  "jevia storage check",
].join("\n");

const STORAGE_OPTIONS = [
  {
    signature: "JSONL · default",
    description: "No setup required. Local history lives in .jevia/runs.jsonl and is ignored by Git.",
  },
  {
    signature: "SQLite · local database",
    description: "A bundled, server-free database for larger local histories and indexed queries.",
  },
  {
    signature: "PostgreSQL · shared evidence",
    description: "Bring a direct or session-pooled database for trusted workspaces that share one project.",
  },
] as const;

const STORAGE_TRANSFER = [
  "jevia storage check",
  "jevia storage check --deep",
  "jevia storage export --output .jevia/snapshot.jsonl",
  "jevia storage import-jsonl --from <file>",
  "jevia storage import-jsonl --from <file> --apply",
].join("\n");

const CACHE_COMMANDS = [
  "jevia cache status",
  "jevia cache clear",
  'jevia route --no-cache "force one live routing decision"',
].join("\n");

const RUN_OPERATIONS = [
  {
    signature: "routed → running → verifying → completed",
    description: "Lifecycle records execution progress separately from whether the task actually succeeded.",
  },
  {
    signature: "runs recover <run-id>",
    description: "Mark an abandoned active run interrupted after confirming no supervisor still owns its lease.",
  },
  {
    signature: "run … --non-interactive",
    description: "Enable process-tree supervision and bounded execution or verification timeouts for headless work.",
  },
] as const;

const MAINTENANCE_COMMANDS = [
  "# Preview and repair a recoverable JSONL tail",
  "jevia runs repair",
  "jevia runs repair --apply",
  "",
  "# Preview and archive older eligible terminal records",
  "jevia runs archive --keep 1000",
  "jevia runs archive --keep 1000 --apply",
].join("\n");

export default function DocsPage() {
  return (
    <main className="site-shell docs-shell">
      <SiteHeader current="docs" />

      <div className="docs-frame page-frame">
        <DocsSidebar />

        <article className="docs-content">
          <header className="docs-hero" id="overview">
            <p>
              Jevia is an adaptive, outcome-based router for coding harnesses. Start
              with the CLI, connect the models and agents you already use, then feed
              verified results back into later routing decisions.
            </p>
          </header>

          <section className="docs-section" id="install" aria-labelledby="install-title">
            <div className="docs-section-heading">
              <span>01</span>
              <div>
                <h2 id="install-title">Install and validate</h2>
                <p>Install the CLI, create local policy, and verify one live Jev request.</p>
              </div>
            </div>
            <CodeBlock code={CLI_SETUP} label="CLI quickstart" language="bash" />
            <div className="docs-note">
              <strong>Two diagnostics</strong>
              <p>
                <code>jevia doctor</code> checks local configuration and storage without
                calling Jev. <code>jevia check</code> performs one live routing round trip
                without saving a synthetic run.
              </p>
            </div>
          </section>

          <section className="docs-section" id="quickstart" aria-labelledby="quickstart-title">
            <div className="docs-section-heading">
              <span>02</span>
              <div>
                <h2 id="quickstart-title">Route, run, and inspect</h2>
                <p>Use routing on its own, or let Jevia supervise the complete harness workflow.</p>
              </div>
            </div>
            <CodeBlock code={CLI_WORKFLOW} label="Core CLI workflow" language="bash" />
            <div className="docs-methods docs-methods-spaced" role="list">
              {CLI_COMMANDS.map((command) => (
                <div className="docs-method" role="listitem" key={command.signature}>
                  <code>{command.signature}</code>
                  <p>{command.description}</p>
                </div>
              ))}
            </div>
            <div className="docs-note">
              <strong>Automation</strong>
              <p>
                Add <code>--json</code> where supported for stable machine-readable output.
                Run <code>jevia &lt;command&gt; --help</code> for every command-specific option.
              </p>
            </div>
          </section>

          <section className="docs-section" id="adaptive" aria-labelledby="adaptive-title">
            <div className="docs-section-heading">
              <span>03</span>
              <div>
                <h2 id="adaptive-title">How adaptive routing works</h2>
                <p>Routing and task success stay separate until a trusted result closes the loop.</p>
              </div>
            </div>
            <div className="docs-methods" role="list">
              {ADAPTIVE_LOOP.map((step) => (
                <div className="docs-method" role="listitem" key={step.signature}>
                  <code>{step.signature}</code>
                  <p>{step.description}</p>
                </div>
              ))}
            </div>
            <div className="docs-note">
              <strong>Verified, not self-scored</strong>
              <p>
                Jevia does not decide that its own output is good. A completed verifier
                or explicit human feedback supplies the outcome used as learning evidence.
              </p>
            </div>
          </section>

          <section className="docs-section" id="adapters" aria-labelledby="adapters-title">
            <div className="docs-section-heading">
              <span>04</span>
              <div>
                <h2 id="adapters-title">Connect any harness</h2>
                <p>Map stable capability tiers to the model names your harness accepts.</p>
              </div>
            </div>
            <CodeBlock code={HARNESS_SETUP} label="Preview a harness adapter" language="bash" />
            <p className="docs-body-copy">
              This generic adapter is a template, not a provider preset. Replace the command,
              arguments, and model IDs with values your harness accepts. The preview is offline
              and changes no files.
            </p>
            <h3 className="docs-subheading">Apply, validate, and run</h3>
            <CodeBlock code={HARNESS_COMMANDS} label="Harness workflow" language="bash" />
            <p className="docs-body-copy">
              Changing an existing adapter also requires <code>--replace</code>. Extra arguments
              for the configured harness can follow <code>--</code> on <code>jevia run</code>.
              Jevia executes argument arrays directly without shell expansion.
            </p>
            <div className="docs-note">
              <strong>Agnostic by design</strong>
              <p>
                This works with Claude Code, Codex, OpenCode, Gemini CLI, Cursor Agent,
                Copilot CLI, Aider, Goose, Amp, or a custom runner. Jevia learns from
                outcomes, while the adapter owns the tier-to-model mapping.
              </p>
            </div>
          </section>

          <section className="docs-section" id="methods" aria-labelledby="methods-title">
            <div className="docs-section-heading">
              <span>05</span>
              <div>
                <h2 id="methods-title">Use the Node.js SDK</h2>
                <p>Embed the same routing and feedback loop directly inside a harness.</p>
              </div>
            </div>
            <CommandBlock command="npm install jevia" label="Install the Node.js SDK" />
            <CodeBlock code={SDK_EXAMPLE} label="Programmatic harness integration" />
            <p className="docs-body-copy">
              The SDK invokes the local CLI through its shell-free JSON interface. Install
              the CLI first and keep it on <code>PATH</code>; npm installation never runs a
              binary downloader. Pass an <code>AbortSignal</code> in method options when the
              caller needs cancellation.
            </p>
            <div className="docs-methods docs-methods-spaced" role="list">
              {METHODS.map((method) => (
                <div className="docs-method" role="listitem" key={method.signature}>
                  <code>{method.signature}</code>
                  <p>{method.description}</p>
                </div>
              ))}
            </div>

            <h3 className="docs-subheading">Configure storage from Node.js</h3>
            <div className="docs-note docs-note-flush">
              <strong>Unreleased Node API</strong>
              <p>
                These methods are pending the next npm release and are not in
                <code> jevia@0.1.0</code>. They require CLI 0.1.2 or newer and an
                existing <code>jevia init</code> project. Creating a client never
                connects to a database or changes storage.
              </p>
            </div>
            <CodeBlock code={SDK_STORAGE_SETUP} label="SDK: SQLite setup" />
            <CodeBlock code={SDK_POSTGRES_SETUP} label="SDK: PostgreSQL setup" />
            <p className="docs-body-copy">
              Setup stays preview-first and returns human-readable reports. Keep database
              credentials in the named environment variable, use <code>signal</code> for
              cancellation, and raise the client&apos;s <code>timeoutMs</code> for large imports.
              After cancellation or failure, inspect the config and destination before retrying.
            </p>

            <h3 className="docs-subheading">Cancellation and errors</h3>
            <CodeBlock code={SDK_ERROR_HANDLING} label="SDK error handling" />
            <div className="docs-note">
              <strong>Log the safe fields</strong>
              <p>
                Log the message, exit code, and signal. Raw command, stdout, and stderr can
                contain sensitive values and are for private debugging only. Invalid JSON or
                invalid complete records raise <code>JeviaProtocolError</code>.
              </p>
            </div>
          </section>

          <section className="docs-section" id="outcomes" aria-labelledby="outcomes-title">
            <div className="docs-section-heading">
              <span>06</span>
              <div>
                <h2 id="outcomes-title">Verify and report outcomes</h2>
                <p>Only trusted, known results teach later routing decisions.</p>
              </div>
            </div>
            <CodeBlock code={OUTCOME_COMMANDS} label="Record and inspect feedback" language="bash" />
            <div className="docs-methods docs-methods-spaced" role="list">
              {EVIDENCE_TYPES.map((evidence) => (
                <div className="docs-method" role="listitem" key={evidence.signature}>
                  <code>{evidence.signature}</code>
                  <p>{evidence.description}</p>
                </div>
              ))}
            </div>
            <div className="docs-note">
              <strong>Corrections stay visible</strong>
              <p>
                Changing a known outcome requires <code>--reason</code>. Jevia keeps the
                prior value in its feedback history. Setting <code>unknown</code> removes
                the run from learning without erasing its execution evidence.
              </p>
            </div>
          </section>

          <section className="docs-section" id="storage" aria-labelledby="storage-title">
            <div className="docs-section-heading">
              <span>07</span>
              <div>
                <h2 id="storage-title">Choose and configure storage</h2>
                <p>Start with local JSONL, then move deliberately when your history needs more.</p>
              </div>
            </div>
            <div className="docs-methods" role="list">
              {STORAGE_OPTIONS.map((option) => (
                <div className="docs-method" role="listitem" key={option.signature}>
                  <code>{option.signature}</code>
                  <p>{option.description}</p>
                </div>
              ))}
            </div>

            <h3 className="docs-subheading">Move a project to SQLite</h3>
            <CodeBlock code={SQLITE_SETUP} label="SQLite setup" language="bash" />
            <p className="docs-body-copy">
              Omit <code>--import-jsonl</code> when the current JSONL history is empty.
              The source file is never deleted or rewritten, and relative database paths
              resolve from the discovered project root.
            </p>

            <h3 className="docs-subheading">Share evidence with PostgreSQL</h3>
            <CodeBlock code={POSTGRES_SETUP} label="PostgreSQL setup" language="bash" />
            <p className="docs-body-copy">
              Keep credentials in the environment, never in config or CLI arguments. Pass
              <code>--url-env MY_DATABASE_URL</code> to name a different variable. Jevia
              requires a direct or session-pooled connection, not a transaction-mode pooler.
            </p>

            <h3 className="docs-subheading">Check, export, and import</h3>
            <CodeBlock code={STORAGE_TRANSFER} label="Storage tools" language="bash" />
            <div className="docs-note">
              <strong>Preview before apply</strong>
              <p>
                Setup and import commands do no writes until <code>--apply</code>. A deep
                check reads complete history without a write probe or automatic repair.
                Stop writers before migration and keep the source as a recovery copy.
              </p>
            </div>
          </section>

          <section className="docs-section" id="errors" aria-labelledby="errors-title">
            <div className="docs-section-heading">
              <span>08</span>
              <div>
                <h2 id="errors-title">Cache, diagnostics, and recovery</h2>
                <p>Keep routing responsive while preserving explicit operational control.</p>
              </div>
            </div>

            <h3 className="docs-subheading docs-subheading-first">Routing cache</h3>
            <CodeBlock code={CACHE_COMMANDS} label="Cache controls" language="bash" />
            <p className="docs-body-copy">
              The default cache holds 256 decisions for 15 minutes. Each hit still gets a
              fresh run ID. New outcomes and policy, model, or harness changes produce new
              cache keys; cache errors fail open to a live routing request.
            </p>

            <h3 className="docs-subheading">Run lifecycle and recovery</h3>
            <div className="docs-methods" role="list">
              {RUN_OPERATIONS.map((operation) => (
                <div className="docs-method" role="listitem" key={operation.signature}>
                  <code>{operation.signature}</code>
                  <p>{operation.description}</p>
                </div>
              ))}
            </div>
            <CodeBlock
              code={'jevia run agent "fix the parser" --non-interactive --timeout-seconds 300 --verification-timeout-seconds 120'}
              label="Bounded headless run"
              language="bash"
            />

            <h3 className="docs-subheading">History maintenance</h3>
            <CodeBlock code={MAINTENANCE_COMMANDS} label="Preview-first maintenance" language="bash" />
            <div className="docs-note">
              <strong>Nothing is automatic</strong>
              <p>
                Repair is JSONL-only. Archive works with every backend and writes recovery
                files before removing eligible old records. Neither command applies changes
                until you repeat the reviewed command with <code>--apply</code>.
              </p>
            </div>
          </section>
        </article>

        <footer className="docs-footer">
          <div className="docs-footer-spacer" aria-hidden="true" />
          <div className="docs-footer-content">
            <span>Need a portable version of this guide?</span>
            <a href="/docs.md">
              View docs.md
              <ArrowUpRight size={14} strokeWidth={1.7} aria-hidden="true" />
            </a>
          </div>
          <span className="frame-junctions docs-footer-junctions" aria-hidden="true" />
          <span className="docs-footer-sidebar-junction" aria-hidden="true" />
        </footer>
      </div>
    </main>
  );
}
