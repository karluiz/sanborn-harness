# Sanborn harness

## Run `sb -p`

Requires Rust 1.88+ and an Azure deployment compatible with Chat Completions
and function calling. Dependencies are pinned in `Cargo.lock`.

```sh
cargo install --path harness/crates/sanborn-cli --locked
sb init
sb -p "List the visible files and summarize the README" -w /path/to/approved-workspace
sb -p "Create greeting.txt containing hello, then read it back" \
  -w /path/to/disposable-workspace --allow-write
```

For development without installation:

```sh
cargo run --manifest-path harness/Cargo.toml -p sanborn-cli -- \
  -p "Inspect the visible files" -w /path/to/approved-workspace
```

Only use a workspace whose contents are approved for your inference endpoint.
The prompt, tool schemas and selected tool results leave the machine. Metadata
traces omit tool inputs and file contents, but the model's final answer can quote
them. This is not a sandbox for hostile tasks or concurrently modified files.

## Local configuration

Sanborn is the Rust harness **and** Laya together. `sb init` owns setup for
both; no separate Laya installation or command is required for the current
local evaluator.

```sh
sb init --endpoint https://your-resource.cognitiveservices.azure.com/ \
  --deployment your-deployment
sb init --check-inference
```

Before initialization, provision the API key through your secret manager as
`AZURE_OPENAI_API_KEY` or the Sanborn keyring entry described below. Init
verifies credential availability without printing or copying the key.
It validates inference configuration and runs Laya's local
Continue/Replan/Absent readiness checks. It never reads workspace files.
No network request is made unless `--check-inference` is supplied; that flag
sends one synthetic chat probe, not a certification of function-call support.
Failed checks do not save configuration.

User configuration is stored atomically as JSON in
`$XDG_CONFIG_HOME/sanborn/config.local`, or
`~/.config/sanborn/config.local` when XDG is unset. The file is mode 0600 on
Unix and contains only configuration version, inference endpoint, deployment,
API version and required Laya backend (`local-evidence`), never a credential.
It belongs to the user, not the workspace, and creates no repository files.
Running `sb init` again with unchanged settings is idempotent. Changing existing
settings requires `--force`; use explicit endpoint/deployment flags with force,
which rebuilds settings from flags, environment/keyring and defaults. It can
also repair malformed configuration. Unsupported versions or Laya backends
fail explicitly during normal execution.

For task execution, configuration precedence is CLI flags, Azure environment
variables, saved user configuration, then the endpoint keyring/default
deployment and API version. Existing environment/keyring-only `sb -p` usage
remains supported before init. Laya is required in either path and runs its
readiness checks before task execution. Future backend prerequisites belong
in `sb init` when that backend is implemented; init currently does not download
models, select a learned backend, or claim semantic calibration.

The CLI checks `AZURE_OPENAI_API_KEY` first; otherwise it retrieves a Secret
Service entry with attributes `application=sanborn`, `service=azure-inference`.
The endpoint comes from `--endpoint`, `AZURE_OPENAI_ENDPOINT`, saved init
configuration, or a separate
keyring entry with `application=sanborn`, `service=azure-inference-endpoint`.
Linux keyring access uses `secret-tool`; other platforms can use environment
configuration supplied by their secret manager.

`AZURE_OPENAI_DEPLOYMENT` / `--deployment` defaults to `gpt-5.4-mini`;
`AZURE_OPENAI_API_VERSION` / `--api-version` defaults to `2024-10-21`.
Use the deployment name, not a model-family alias. Compatibility with other
deployments is not implied.

Provision values through your secret manager or keyring UI. Do not paste API
keys into command arguments, shell history, `.env` files in this repository,
documentation, or commits. Account-specific infrastructure is not hardcoded.
HTTPS is required, redirects are disabled, and HTTP error bodies are not logged.

## Diagnostics and automation

```sh
sb doctor
sb doctor --json -w /path/to/approved-workspace
cat task.txt | sb -p -
sb -p - --json --json-events < task.txt
```

`sb doctor` checks workspace accessibility, saved configuration, local credential
availability, effective inference configuration, and required Laya readiness.
It uses the same configuration precedence as task execution and accepts
`--endpoint`, `--deployment`, `--api-version`, and `-w`. It makes no inference
request, runs no workspace tool, and changes no configuration. Missing saved
configuration is acceptable when environment/keyring setup supplies the values;
malformed saved configuration is not silently ignored.

Doctor reports every check, with exit zero only when all local checks pass.
Its versioned JSON report has `schema_version`, `ready`, `network_checked: false`
and `checks` (`name`, `ready`, `detail`). Neither API keys nor resolved endpoint
values are printed. Local readiness does not establish authentication, model
availability, tool compatibility, data-egress approval or OS isolation.

`-p -` reads a prompt from piped stdin or file redirection. It preserves line
breaks and requires nonempty UTF-8 without NUL bytes, at most 128,000 bytes.
The same validation applies to literal prompts. Interactive terminal stdin is
rejected instead of starting an indefinite input session. Stdin acquisition
has its own deadline using `--timeout`; task execution then has its existing
execution deadline. One detached reader thread exists only for stdin mode,
so an open pipe does not prevent process exit after timeout or cancellation.
This is separate from the thread-free terminal animation.

`--json` writes a single final JSON object to stdout and disables terminal
animation/color. Combine it with `--json-events` for metadata events on stderr.
Success fields:

```json
{
  "schema_version": 1,
  "status": "completed",
  "acceptance": "not_certified",
  "final_text": "...",
  "turns": 2,
  "tool_calls": 4,
  "inference_calls": 2,
  "usage_complete": true,
  "usage": {
    "input_tokens": 0,
    "output_tokens": 0,
    "cache_read_tokens": 0,
    "cache_write_tokens": 0
  },
  "duration_ms": 0,
  "stop_reason": "end_turn",
  "laya_backend": "local-evidence"
}
```

Values above are illustrative. `duration_ms` covers the agent execution,
not stdin, setup, or process startup. Runtime failures exit nonzero and emit
`schema_version`, `status: "error"`, `acceptance: "not_certified"`, `error`, and
a note that completed writes are not rolled back. Error results do not include
tool counts. They include observed shared inference calls and usage once the
provider is initialized, or null beforehand. `usage_complete` is false if any
reserved request failed or was cancelled; charges for those requests may still
exist at the provider. Argument-parsing errors retain Clap's stderr output;
they do not enter the JSON runtime contract. Error diagnostics also go to stderr
and are not JSON event records. Final text may contain workspace content even
when event traces omit it.

## Execution architecture

```text
sb -p -> Azure reasoning -> bounded plan -> Rust validation
      -> Laya scheduling decision -> Rust permission gate -> workspace tool
      -> Laya evidence decision -> satisfied dependencies / explicit replan
      -> remaining ready steps -> reasoning for verification and final answer
```

Laya is a **required component** of the planned runner, not a bypassable plugin.
`sanborn-laya` owns typed `ContinuePlannedStep`, `Replan`, and `Absent` decisions,
the decision-engine interface, and evidence guards. The initial `LocalLaya`
evaluator checks exact local evidence. It performs no HTTP request and makes no
probabilistic or semantic-confidence claim. How a learned backend is embedded
will be defined separately; it must preserve this contract.

Each step declares fixed tool input, earlier dependencies, and an evidence guard:
`tool_succeeded`, `text_equals`, or `text_contains`. Rust validates the whole plan
before executing anything, enforces tool permissions independently, and checks
evidence even if a faulty decision engine recommends continuation. Missing or
uncertain decisions interrupt the plan and return their reason and executed
results to reasoning; they do not bypass Laya and execute the remaining steps.

Consecutive independent reads run with a concurrency ceiling of eight. Writes
are serialized, and dependencies must be satisfied before a step is scheduled.
Each plan has at most 16 steps. A completed plan is not proof of task completion:
reasoning must inspect the evidence, request verification where needed, and
produce a final response. The CLI does not independently certify arbitrary
natural-language acceptance criteria.

The original conventional `Agent` remains available for offline comparison.
`sb` uses `PlannedAgent`; there is no CLI switch to disable Laya.

## Tadpoles: summon, investigate, croak

The parent can request `summon_tadpoles` in a bounded plan for independent,
narrow read-only investigations. No separate setup is needed: tadpoles share
the parent's configured inference provider, workspace boundary and required
Laya backend. Laya gates parent scheduling, each worker's dispatch, and every
child tool's scheduling and evidence. Rust still validates every child plan.

```sh
sb -p "Summon two tadpoles: one to inspect the README, one to inspect visible test files. Compare their evidence." \
  -w /path/to/approved-workspace --max-tadpoles 3 --max-inference-calls 24
```

The model decides whether to summon; a prompt is not a guarantee of delegation.
Each group contains one to three concurrent workers; groups execute serially
relative to other parent tools. IDs are unique across the run. The default
run cap is three tadpoles (`--max-tadpoles`, range 1–8). Each worker has four
reasoning turns, eight tool calls, 2,048 output tokens per response and a
60-second deadline. Its registry contains only read tools: no writes, shell
execution or recursive summoning, even with parent `--allow-write`.

Parent and children share a 24-call inference ceiling
(`--max-inference-calls`, range 1–200). Requests reserve capacity before awaiting,
including requests that fail or are cancelled; no automatic retries occur.
`--max-turns`, `--max-tool-calls` and `--max-tokens` bound the parent; the fixed
worker budgets are additional. Final JSON and the parent completion event
include child token usage and tool counts. Usage sums received responses,
not an authoritative bill; failed/cancelled requests can still cost tokens.
`usage_complete` makes missing response usage explicit.

Each croak has a worker ID and either `returned` (final model report with
bounded tool evidence) or `blocked` (explicit runtime/Laya/deadline reason).
`returned` does not mean the objective was proven. Every croak is
`acceptance: "not_certified"`; the parent must evaluate and verify findings.
Evidence/report size is bounded; truncation is marked, and evidence is
explicitly omitted if combined serialized output exceeds the tool budget.
Cancelling the parent drops child request futures, but does not guarantee
remote inference or already-started filesystem reads stop immediately.

Feedback uses **Summoning** and **Croak [worker]** markers.
Metadata events are `summoned`, `tadpole` (ID plus nested `detail` event),
and `croak`. They omit objectives, reports and file contents.
This demonstrates bounded Laya-guided delegation, not a measured speedup.

### Specialized contracts, not just specialist prompts

`summon_tadpoles` accepts optional `specialty` and `scope` fields. Omit
`specialty` (or use `general`) to preserve ordinary workers. Built-in v1
contracts are `repository_scout` and `test_analyst`; unknown names fail.
The parent chooses a contract explicitly; there is no learned role router.

```json
{
  "tadpoles": [
    {"id": "map", "specialty": "repository_scout", "scope": ".",
     "objective": "Map the visible repository and cite relevant paths"},
    {"id": "tests", "specialty": "test_analyst", "scope": "tests",
     "objective": "Inspect test evidence and explain limitations"}
  ]
}
```

`scope` defaults to the workspace root and must resolve to an existing
workspace directory. Specialist read/list/search tools enforce that boundary;
writes, commands, recursive delegation and extra tools are excluded.

The scout has a Rust-authored `list_files` baseline. The analyst has a
Rust-authored `list_files` plus `search_files` baseline with fixed heuristic
test markers (Rust attributes, JS/TS test calls, Python and Go test names).
These steps execute **before the child's first reasoning request**, using the
same plan validator, Laya scheduling/evidence decisions and independent Rust
guards. Their calls count against the existing eight-tool child budget.
The analyst's two independent discovery reads can run in parallel.

The `discovery` guard validates structured file-list/search output.
Limited/truncated discovery or skipped search files yield insufficient evidence;
malformed data fails. Empty matches are valid discovery output, but only mean
no matching signals—not no tests. Hidden/ignored files and traversal depth
remain outside the declared discovery surface.

Reasoning receives the observed baseline and handles interpretation,
additional scoped reads or replanning within remaining budgets. It cannot
turn an incomplete fixed baseline into a satisfied contract through prose.
A specialized croak includes `specialty`, `contract.version`,
`contract.baseline_complete`, bounded facts/counts/samples and an interruption
when present. Missing baseline evidence produces a blocked croak with the
model's report retained. Rerun a narrower scope when necessary; extra model
steps do not retroactively repair that baseline.

`baseline_complete` means required guarded discovery completed within its
visible bounded scope. It does not certify the objective, infer architecture,
measure coverage, execute tests or prove absence of tests. The report remains
model-authored and the parent must verify it. Metadata adds a child
`contract_started` event naming its specialty without exposing scope or objective.
Offline fixtures demonstrate both contracts returning after one reasoning call
per child; no representative comparative performance result is established.

## Tools and boundaries

| Tool | Behavior |
| --- | --- |
| `read_file` | UTF-8 regular file, at most 128,000 bytes |
| `list_files` | Visible, non-ignored recursive entries; no followed symlinks |
| `search_files` | Bounded Rust-regex search; binary/oversized skips and limits reported |
| `write_file` | Opt-in atomic create or compare-before-replace; existing parent required |
| `summon_tadpoles` | Bounded read-only Laya workers; structured noncertified croaks |

Writes require `--allow-write`. For creation, `expected_content` must be null.
For replacement, it must exactly match the current contents. Replacement keeps
the existing permissions. No shell, delete, or arbitrary command tool exists.
File tools reject workspace escapes, external/dangling symlinks and common
credential-like paths such as `.env`, `.git`, private-key extensions and `.local`
files. These checks are not comprehensive secret detection or race-proof
filesystem isolation. Hidden and ignored files are omitted from traversal.

Defaults: 12 reasoning turns, 64 tool calls, 4,096 output tokens per model call,
60 seconds per inference request, 10 seconds per read tool, and a 180-second
execution deadline. CLI flags can reduce or increase bounded run limits; see
`sb --help`. Ctrl+C cancels the run. Completed writes are not rolled back.
In-flight blocking filesystem work may finish after cancellation; writes are
not individually timed out and replanned while their worker is still running.
These are cooperative limits, not hard OS-level execution deadlines.

`--json-events` writes metadata-only JSON lines to stderr; the final answer is
on stdout. Events identify reasoning calls, plans, Laya decisions, tools,
durations and usage. Exit zero means the model returned a final answer without
a runtime error, not that an external acceptance test passed.

## Terminal feedback

Interactive stderr gets green/cyan status markers and an ASCII `| / - \`
spinner with elapsed time while setup, reasoning or tools are pending.
Waiting animation ticks eight times per second on the existing async runtime;
status events update immediately. It creates no thread, model request, or new dependency. There is no
artificial loading delay. Laya continuation chatter is condensed into phase
feedback; replans, absences, tool errors and the final usage summary stay visible.

Color is inspired by Crayon's terminal styling, not an R runtime integration.
`NO_COLOR` disables colors (including when empty). `--plain` disables both
color and animation. Pipes, `TERM=dumb`, and `--json-events` disable both
automatically. Stdout stays reserved for final answers or init results.
The active line is cleared on normal completion, errors, deadlines and Ctrl+C;
no cursor hiding or alternate screen is used.

Opt-in release-mode renderer benchmarks separate formatting-to-sink cost
from actual stderr writes. Terminal and SSH speed can still dominate writes;
`--plain` removes animation when that overhead is undesirable. These benchmarks
are not evidence of harness or Laya inference speedup.

## Validation

```sh
cargo test --manifest-path harness/Cargo.toml --workspace --locked
cargo clippy --manifest-path harness/Cargo.toml --workspace --all-targets --locked -- -D warnings
```

Offline tests cover Azure wire translation, mocked HTTP authentication/errors,
plan dependencies, policy denial, Laya absence, faulty-backend guard rejection,
bounded read concurrency, serialized mutations, tool budgets and read timeouts,
path escapes, protected paths, and guarded file writes. Tadpole fixtures cover
concurrent workers, shared inference accounting/budget exhaustion, parent
handoff, Laya absence, forbidden registries/tools, deadlines and cancellation.

An explicitly opt-in Azure smoke test sends only synthetic fixtures:

```sh
cargo test --manifest-path harness/Cargo.toml -p sanborn-cli \
  --test azure_smoke -- --ignored
```

It requests four tools in one plan and asserts exact file contents, eight Laya
decisions and two reasoning calls. This demonstrates the mechanism, not a
comparative speedup. Learned decision inference, confidence calibration,
caching/invalidation, streaming, context compaction, durable recovery, shell
isolation and a representative A/B benchmark remain future work.
