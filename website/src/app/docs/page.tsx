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

const WINDOWS_CLI_SETUP = [
  "irm https://jevia.dev/install.ps1 | iex",
  "jevia --version",
  "jevia init",
  '$env:TYPESAFE_API_KEY = "your-key"',
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
  "// Optional: your adapter may expose a known outcome; no verifier is required.",
  'if (result.outcome === "success" || result.outcome === "failure") {',
  "  await jevia.feedback(route.run_id, result.outcome);",
  "}",
  "",
  "// Recorded outcomes are included automatically in the next decision.",
  'const next = await jevia.route("fix another parser regression");',
].join("\n");

const SDK_STORAGE_SETUP = [
  "// Relative paths resolve from the client cwd; absolute paths also work.",
  'const target = { backend: "sqlite", path: ".data/jevia/history.db" } as const;',
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
    description: "Choose a capability tier using recent eligible recorded outcomes automatically, and return the typed route record.",
  },
  {
    signature: "feedback(runId, outcome, options?)",
    description: "Optionally record an application-reported outcome. No verifier is required; omitting feedback leaves the outcome unknown.",
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
    signature: "complete(runId, outcome, options)",
    description: "Finish externally executed work after confirming it has stopped; requires CLI 0.1.4 or newer.",
  },
  {
    signature: "setupStorage(target, options?)",
    description: "Preview SQLite or PostgreSQL setup and apply it only with confirmation.",
  },
  {
    signature: "checkStorage(options?)",
    description: "Check selected storage or deeply validate it without a write probe.",
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
    description: "A configured or detected verifier completed and produced a known result. Eligible for learning.",
  },
  {
    signature: "manual",
    description: "An application or person explicitly recorded success or failure. Eligible for learning.",
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
  "# Choose a project-relative or absolute SQLite file; preview writes nothing",
  "jevia storage setup sqlite \\",
  "  --path .data/jevia/history.db \\",
  "  --import-jsonl",
  "",
  "# Stop Jevia writers and supervisors, then apply the same reviewed path",
  "jevia storage setup sqlite \\",
  "  --path .data/jevia/history.db \\",
  "  --import-jsonl --apply --confirm-stopped",
  "jevia storage check",
  "jevia stats",
].join("\n");

const SQLITE_CONFIG = [
  "[storage]",
  'backend = "sqlite"',
  'url = "sqlite://.data/jevia/history.db"',
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
    description: "Defaults to .jevia/jevia.db; choose another project-relative or absolute file with --path.",
  },
  {
    signature: "PostgreSQL · shared evidence",
    description: "Records live in your configured database under the selected project namespace.",
  },
] as const;

const STORED_RUN = [
  "{",
  '  "schema_version": 3,',
  '  "run_id": "7b65a69a-0a6f-4a89-bd73-88f090954dd9",',
  '  "tier": "balanced",',
  '  "suggested_tier": "balanced",',
  '  "confidence": 0.84,',
  '  "probabilities": {',
  '    "fast": 0.1,',
  '    "balanced": 0.84,',
  '    "strong": 0.06',
  "  },",
  '  "fallback_applied": false,',
  '  "jev_model": "jev-latest",',
  '  "created_at_ms": 1790700000000,',
  '  "source": "live",',
  '  "task": "fix the flaky integration test",',
  '  "outcome": "success",',
  '  "execution": {',
  '    "harness": "codex",',
  '    "model": "provider/standard",',
  '    "duration_ms": 48231,',
  '    "exit_code": 0,',
  '    "verification": {',
  '      "command": "pnpm",',
  '      "launched": true,',
  '      "duration_ms": 6842,',
  '      "exit_code": 0',
  "    }",
  "  },",
  '  "lifecycle": {',
  '    "state": "completed",',
  '    "started_at_ms": 1790700001120,',
  '    "finished_at_ms": 1790700056193',
  "  },",
  '  "outcome_evidence": {',
  '    "source": "verification",',
  '    "recorded_at_ms": 1790700056193',
  "  }",
  "}",
].join("\n");

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
            <p>
              With <code>jevia run</code>, the CLI handles execution,
              verification, and outcome recording automatically. No manual feedback
              step is needed. See <a href="#adapters">automatic CLI execution</a> below
              for setup, release availability, and verification limits.
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
            <CodeBlock
              code={WINDOWS_CLI_SETUP}
              label="Windows PowerShell quickstart"
              language="powershell"
            />
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
                or explicit application or human feedback supplies the outcome used as learning evidence.
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
            <div className="docs-note">
              <strong>Automatic CLI pipeline</strong>
              <p>
                Configure your installed harness, credentials, and model mappings once.
                Then <code>jevia run</code> routes the task, launches the agent, runs
                verification after a successful exit, and records the outcome in your
                selected storage backend. No manual <code>feedback</code> or
                <code> runs complete</code> step is needed. Verifier-backed outcomes
                become evidence for later routing decisions automatically.
              </p>
            </div>
            <CommandBlock
              command={'jevia run codex "fix the failing test"'}
              label="Run a configured harness"
            />
            <p className="docs-body-copy">
              CLI 0.1.4 detects root Rust workspace tests or Node test scripts when no
              verifier is configured; an explicit verifier takes precedence. Missing
              or ambiguous checks stay unverified and are excluded from learning.
              Passing tests is evidence, not proof of every requirement.
            </p>
            <p className="docs-body-copy">
              Verification runs when the agent process/session finishes, not after each
              internal message or tool call. Jevia does not install the agent or test
              runner, supply credentials, bypass agent permission prompts, or retry
              failed work. Prepare project dependencies first. See the{" "}
              <a href="https://github.com/assistant-ui/jevia/blob/main/docs/reference.md#automatic-cli-pipeline">
                CLI setup and verification reference
              </a> for supported tests, deadlines, and opt-out settings.
            </p>
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
              For SDK integrations, map Jevia&apos;s tier to your harness model and let
              your application run the work. Feedback and verification are optional.
              SDK routing does not launch an agent or run tests. If your application
              knows the result, it can report it without a verifier; no human feedback
              prompt is required. Skipping feedback still records the route with an
              unknown outcome and does not block later routing.
            </p>
            <p className="docs-body-copy">
              Your adapter can call Codex, Claude Code, OpenCode, Gemini CLI, Cursor
              Agent, Copilot CLI, Aider, Goose, Amp, or a custom runner. The optional
              <code> result.outcome</code> field above comes from your own adapter;
              Jevia does not infer it from a successful function return. Report only
              known outcomes, and do not submit feedback again after a supervised CLI run.
            </p>
            <div className="docs-note">
              <strong>Recorded outcomes inform the next route</strong>
              <p>
                The SDK does not require Jevia&apos;s built-in verifier. Your application
                can use tests, acceptance checks, or a user-approved result and record
                success or failure. This is labeled <code>manual</code> evidence
                (application-reported), not CLI verification. Use <code>unknown</code>
                when the result is uncertain. The next <code>route()</code> automatically
                includes eligible recorded outcomes from the same JSONL, SQLite, or
                PostgreSQL history, including CLI-verified results. No manual cache
                clearing is needed: evidence is part of the cache key.
              </p>
            </div>
            <p className="docs-body-copy">
              Unknown, active, and process-exit-only records are excluded.
              <code> [router].history_limit</code> bounds recent evidence: default 20,
              maximum 100, or 0 to disable it. This is decision context, not model
              training or a guarantee of better choices. Live requests send eligible
              historical task text and outcome metadata to Jev; feedback reasons stay
              in storage. <code>[privacy].store_task_text = false</code> omits task text
              from new records, not older history or the current routing request.
            </p>
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
              <strong>Node API availability</strong>
              <p>
                These methods are available in <code>jevia@0.1.1</code>. They require
                CLI 0.1.2 or newer and an existing <code>jevia init</code> project.
                Creating a client never connects to a database or changes storage.
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
            <p className="docs-body-copy">
              These commands are for externally executed work or corrections. A supervised
              <code> jevia run</code> records its outcome automatically; no manual feedback
              step is needed afterward.
            </p>
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

            <h3 className="docs-subheading">What a stored run looks like</h3>
            <p className="docs-body-copy">
              Every backend preserves the same logical record. JSONL writes one compact JSON
              object per line to <code>.jevia/runs.jsonl</code>; this example is expanded only
              for readability. SQLite and PostgreSQL store the equivalent fields while keeping
              the same lifecycle and outcome-evidence semantics.
            </p>
            <CodeBlock
              code={STORED_RUN}
              label="One completed run · expanded JSONL"
              language="json"
            />
            <div className="docs-note">
              <strong>The verifier supplies the outcome</strong>
              <p>
                Jev selects the tier, but it does not declare its own work successful. Here,
                the verifier exited successfully, so <code>outcome_evidence.source</code> is
                <code> verification</code>. Set <code>privacy.store_task_text = false</code>
                to persist <code>task: null</code> instead of the raw task.
              </p>
            </div>

            <h3 className="docs-subheading">Move a project to SQLite</h3>
            <CodeBlock code={SQLITE_SETUP} label="SQLite setup" language="bash" />
            <p className="docs-body-copy">
              Without <code>--path</code>, SQLite uses <code>.jevia/jevia.db</code>. Relative
              paths resolve from the discovered project root, even when the command runs in
              a subdirectory; absolute paths are also accepted. Omit <code>--import-jsonl</code>
              when the current JSONL history is empty. The source file is never deleted or rewritten.
            </p>
            <CodeBlock code={SQLITE_CONFIG} label="Saved in .jevia/config.toml" language="toml" />
            <div className="docs-note">
              <strong>Protect custom locations</strong>
              <p>
                Files under <code>.jevia</code> are ignored automatically. If you choose a
                path elsewhere, add the database, its WAL/SHM sidecars, and run-lock files
                to your ignore rules and protect the containing directory.
              </p>
            </div>

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
            <a href="/api/docs-markdown">
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
