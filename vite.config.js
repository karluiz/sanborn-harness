import { defineConfig } from 'vite';
import { fileURLToPath } from 'node:url';
import { copyFileSync } from 'node:fs';

export default defineConfig({
  plugins: [{
    name: 'manual-diagram-source',
    writeBundle() {
      copyFileSync('docs/architecture.json', 'dist/docs/architecture.json');
      copyFileSync('docs/ARCHIFY-LICENSE.txt', 'dist/docs/ARCHIFY-LICENSE.txt');
      copyFileSync('docs/prism.js', 'dist/docs/prism.js');
      copyFileSync('docs/PRISM-LICENSE.txt', 'dist/docs/PRISM-LICENSE.txt');
    },
  }],
  build: {
    rollupOptions: {
      input: {
        landing: fileURLToPath(new URL('./index.html', import.meta.url)),
        manual: fileURLToPath(new URL('./docs/index.html', import.meta.url)),
        architecture: fileURLToPath(new URL('./docs/architecture.html', import.meta.url)),
      },
    },
  },
});
