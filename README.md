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
