# Sanborn

An interactive Three.js site featuring a low-poly geometric frog on a lily pad, with a bottom-anchored “a Fredycorp Company” footer.

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

Drag the frog (or focus the scene and use the arrow keys) to rotate. Switch between three colors, pause idle motion, reset the view, or use “Say hello” to make Freddy hop. Reduced-motion preferences are respected. The scene requires WebGL; unsupported browsers show an explicit message. Fonts use Google Fonts with local system fallbacks.
