# Sanborn

An interactive Three.js site featuring a low-poly geometric frog on a lily pad, with a bottom-anchored “a Fredycorp Company” footer.

## Rust harness

The first runnable harness lives in [`harness/`](harness/README.md). Install
the `sb` binary with Rust 1.88+:

```sh
cargo install --path harness/crates/sanborn-cli --locked
sb init
sb -p "Inspect the visible files in this workspace"
```

`sb init` initializes user-level inference settings and checks the required
Laya layer; `--check-inference` optionally sends a synthetic readiness probe.
Configuration stays outside the workspace and contains no credentials.
`sb doctor` checks local readiness without inference requests. `sb -p -` reads
bounded prompts from stdin; `--json` returns a versioned final result for scripts.
Read-only **tadpoles** run the same required Laya loop: the parent summons
bounded independent workers and receives structured **croaks** to verify.

Scout and test-analyst croaks can carry structured findings with scoped paths,
line ranges and Rust-attached exact excerpts. Laya gates citation integrity,
not interpretation. Runnable specialist examples check expected fixture
findings as well as runtime boundaries; prose alone is never proof.
`--max-tadpoles` caps workers; `--max-inference-calls` caps parent and child
requests together. No recursive summoning or child writes are allowed.
Specialized `repository_scout` and `test_analyst` tadpoles use Rust-authored
discovery baselines before child reasoning, required typed Laya evidence gates
and enforced directory scope. Their contracts describe observed evidence,
not certified architecture or measured test coverage.
`coder` tadpoles inspect scoped existing files and propose one bounded replacement.
Laya gates proposal evidence; only the parent can apply the saved patch through
`apply_tadpole_patch` with `--allow-write` and an exact old-content check.
V1 supports existing-file edits up to 8,000 bytes per old/new body, not creation,
deletion, multi-file changes or test execution.
`sb` uses Azure reasoning to produce bounded plans and a required local Laya
decision layer to evaluate scheduling and evidence. It is read-only by default;
`--allow-write` enables guarded file writes. Credentials and the endpoint come
from environment configuration or the local system keyring, never source.
Selected tool results are sent to the configured Azure endpoint.

Laya's first implementation uses deterministic evidence guards, not a learned
decision model. Backend embedding, semantic confidence calibration, shell
isolation and comparative speed benchmarks remain unfinished.

## Development

Requires Node.js 20.19+ or 22.12+.

```sh
npm install
npm run dev
```

## Production

```sh
npm run build
npm run preview
```

Deploy the generated `dist/` directory to any static host.

For GitHub Pages, `.github/workflows/pages.yml` deploys the static landing
site from `main`, excluding the Rust harness and dependency files. Enable
Pages with GitHub Actions as its source and set the custom domain to
`sanborn.io`. The repository must be eligible for Pages before deployment;
private repositories require a supported paid GitHub plan.

## Inside Sanborn

The field manual at `/docs/` explains the proposed Rust/Laya architecture
with a Three.js specimen, selectable system components and route highlighting.
It separates the first runnable slice from planned features and unmeasured performance.
Chapters cover installation and `sb init`, local Laya evidence guards, bounded
plans and file tools, terminal feedback controls, inference boundaries,
reproducible checks and the remaining implementation roadmap. No personal
infrastructure or credentials are included.
The dedicated `/docs/#tadpoles` chapter documents summoning, the required Laya
gates, worker permissions, shared budgets and croak verification. Its lazy-loaded
Three.js pond reuses the Sanborn frog and shows geometric tadpoles carrying
instruction scrolls out and result packets home. It is explicitly illustrative,
not a live run. Workers have independent departure and return timings; the
presentation can show one croak received while another tadpole is investigating.
Pause/replay,
reduced-motion behavior, offscreen suspension and a static SVG fallback keep
the presentation usable without continuous animation or WebGL.
Shell, Rust and JSON examples use a self-hosted minimal Prism 1.30.0 bundle
in `docs/prism.js`; its upstream license is `docs/PRISM-LICENSE.txt`.
Regenerate it after updating Prism with:

```sh
cat node_modules/prismjs/components/prism-{core,clike,bash,rust,json}.min.js > docs/prism.js
cp node_modules/prismjs/LICENSE docs/PRISM-LICENSE.txt
```

The terminal transcript keeps its authored status colors. Code remains readable
without JavaScript, and highlighting does not affect the Rust CLI.
The page works as static files on Pages; `npm run build` builds both routes.
Reduced motion, keyboard controls, a full text component reference and a
static SVG specimen keep the documentation usable without WebGL or JavaScript.

The manual links to and embeds `docs/architecture.html`, generated locally
with Archify 3.0.1. Its showcase validation, artifact checks and browser gate
passed. The editable source is `docs/architecture.json`; Archify's license
is preserved in `docs/ARCHIFY-LICENSE.txt`. Generated HTML must be regenerated
from that source rather than hand-edited.

Drag the frog (or focus the scene and use the arrow keys) to rotate. Switch between three colors, pause idle motion, reset the view, or use “Say hello” to make Freddy hop. Reduced-motion preferences are respected. The scene requires WebGL; unsupported browsers show an explicit message. Fonts use Google Fonts with local system fallbacks.
