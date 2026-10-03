import { existsSync } from 'node:fs'
import { resolve } from 'node:path'
import adapter from '@sveltejs/adapter-static'
import { vitePreprocess } from '@sveltejs/vite-plugin-svelte'

const target = process.env.SWORM_TARGET ?? 'desktop'
if (target !== 'desktop' && target !== 'web') {
  throw new Error(`Invalid SWORM_TARGET "${target}": expected desktop or web`)
}

const routes = `src/routes-${target}`
if (!existsSync(resolve(routes))) {
  throw new Error(`SWORM_TARGET=${target} requires ${routes}; web routes are not available until Phase 2`)
}
// Targets run side by side in dev, so each keeps its own generated route manifest.
const suffix = target === 'desktop' ? '' : `-${target}`

/** @type {import('@sveltejs/kit').Config} */
const config = {
  preprocess: vitePreprocess(),
  kit: {
    files: { routes },
    outDir: `.svelte-kit${suffix}`,
    adapter: adapter({
      pages: `build${suffix}`,
      assets: `build${suffix}`,
      fallback: 'index.html'
    })
  }
}

export default config
