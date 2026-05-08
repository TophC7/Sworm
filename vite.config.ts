import { sveltekit } from '@sveltejs/kit/vite'
import { resolve, sep } from 'node:path'
import tailwindcss from '@tailwindcss/vite'
import { defineConfig } from 'vite'

// Tauri expects a fixed port during dev
const host = process.env.TAURI_DEV_HOST
const rootDir = process.cwd()
const uiWatchRoots = ['src', 'static'].map((path) => resolve(rootDir, path))

function isUiWatchPath(path: string): boolean {
  const absPath = resolve(path)
  if (absPath === rootDir) return true
  return uiWatchRoots.some((watchRoot) => {
    const isWatchRoot = absPath === watchRoot
    const isInsideWatchRoot = absPath.startsWith(`${watchRoot}${sep}`)
    const isAncestorOfWatchRoot = watchRoot.startsWith(`${absPath}${sep}`)
    return isWatchRoot || isInsideWatchRoot || isAncestorOfWatchRoot
  })
}

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
      // Dev webview should reload only for files that can change UI content.
      ignored: (path) => !isUiWatchPath(path)
    }
  }
})
