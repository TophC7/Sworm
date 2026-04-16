import { sveltekit } from '@sveltejs/kit/vite'
import tailwindcss from '@tailwindcss/vite'
import { defineConfig } from 'vite'

// Tauri expects a fixed port during dev
const host = process.env.TAURI_DEV_HOST
const ignoredWatchPaths = [
  '**/src-tauri/**',
  '**/.direnv/**',
  '**/.git/**',
  '**/.bun/**',
  '**/target/**',
  '**/result/**',
  '**/result-*/**'
]

export default defineConfig({
  plugins: [tailwindcss(), sveltekit()],

  // Prevent vite from obscuring Rust errors
  clearScreen: false,

  // Pre-bundle Monaco worker entry points so dev doesn't re-scan them on each load.
  optimizeDeps: {
    include: [
      'monaco-editor/esm/vs/editor/editor.worker',
      'monaco-editor/esm/vs/language/typescript/ts.worker',
      'monaco-editor/esm/vs/language/json/json.worker',
      'monaco-editor/esm/vs/language/css/css.worker',
      'monaco-editor/esm/vs/language/html/html.worker'
    ]
  },

  build: {
    rollupOptions: {
      output: {
        // Isolate Monaco into its own chunk — loaded lazily on first editor tab.
        manualChunks(id) {
          if (id.includes('monaco-editor')) return 'monaco'
        }
      }
    }
  },

  server: {
    port: 1420,
    strictPort: true,
    host: host || '127.0.0.1',
    hmr: host
      ? {
          protocol: 'ws',
          host,
          port: 1421
        }
      : undefined,
    watch: {
      // Avoid reload storms from local tooling and Nix metadata trees.
      ignored: ignoredWatchPaths
    }
  }
})
