<script lang="ts">
  import './markdown.css'
  import { onDestroy, tick } from 'svelte'
  import { renderMarkdown } from './renderMarkdown'
  import { resolveMarkdownLocalPath, URL_SCHEME_RE } from '$lib/utils/mediaAssets'
  import { platform, type AssetHandle } from '$lib/platform'
  import { openLink } from '$lib/features/workbench/links/openLink'
  import { getErrorMessage } from '$lib/utils/client-error'

  let {
    source,
    folderPath,
    filePath
  }: {
    source: string
    folderPath?: string
    filePath?: string | null
  } = $props()

  let rendered = $state.raw<Awaited<ReturnType<typeof renderMarkdown>> | null>(null)
  let error = $state<string | null>(null)
  let root: HTMLDivElement

  // Document-scoped stream-backed images keyed by resolved project path; entries outlive source edits.
  interface ImageEntry {
    controller: AbortController
    url: Promise<string | null>
    handle: AssetHandle | null
  }
  const images = new Map<string, ImageEntry>()
  let generation = 0
  let context: string | undefined

  function disposeImage(entry: ImageEntry) {
    entry.controller.abort()
    entry.handle?.dispose()
    entry.handle = null
  }

  function invalidateImages() {
    generation++
    for (const entry of images.values()) disposeImage(entry)
    images.clear()
  }

  function loadImage(path: string, load: (signal: AbortSignal) => Promise<AssetHandle>): ImageEntry {
    const controller = new AbortController()
    const entry: ImageEntry = { controller, url: Promise.resolve(null), handle: null }
    entry.url = load(controller.signal).then(
      (handle) => {
        // Pruned or invalidated while pending: the late result is never published.
        if (controller.signal.aborted) {
          handle.dispose()
          return null
        }
        entry.handle = handle
        return handle.url
      },
      (cause) => {
        // Failures fall back for this render only; a later render retries.
        if (images.get(path) === entry) images.delete(path)
        throw cause
      }
    )
    images.set(path, entry)
    return entry
  }

  $effect(() => {
    const nextContext = JSON.stringify([folderPath ?? null, filePath ?? null])
    if (context !== undefined && context !== nextContext) {
      invalidateImages()
      rendered = null
      error = null
    }
    context = nextContext

    const current = ++generation
    const used = new Set<string>()
    const folder = folderPath
    const assets = platform.assets
    const loadLocalImage =
      folder && 'load' in assets
        ? (path: string) => {
            // A superseded render cannot start new loads.
            if (current !== generation) return Promise.resolve(null)
            used.add(path)
            return (images.get(path) ?? loadImage(path, (signal) => assets.load(folder, path, signal))).url
          }
        : undefined
    // Keep displayed URLs alive until the next document replaces their nodes.
    renderMarkdown(source, folderPath, filePath, loadLocalImage).then(
      async (result) => {
        if (current !== generation) return
        rendered = result
        error = null
        await tick()
        if (current !== generation) return
        for (const [path, entry] of images) {
          if (used.has(path)) continue
          disposeImage(entry)
          images.delete(path)
        }
        for (const [index, image] of result.images.entries()) {
          void image.then((url) => {
            if (current !== generation) return
            const node = root.querySelector<HTMLImageElement>(`img[data-sworm-image="${index}"]`)
            if (!node) return
            if (url === null) {
              const fallback = document.createElement('span')
              fallback.className = 'text-muted'
              fallback.textContent = `Local image unavailable${node.alt ? `: ${node.alt}` : ''}`
              node.replaceWith(fallback)
            } else {
              node.src = url
              node.removeAttribute('aria-busy')
              node.removeAttribute('data-sworm-image')
            }
          })
        }
      },
      (cause) => {
        if (current === generation) error = getErrorMessage(cause)
      }
    )
  })

  onDestroy(invalidateImages)

  function handleClick(event: MouseEvent) {
    const anchor = event.target instanceof Element ? event.target.closest('a') : null
    const href = anchor?.getAttribute('href')
    if (!anchor || !root.contains(anchor) || !href) return
    event.preventDefault()

    if (href.startsWith('#')) {
      let id = href.slice(1)
      try {
        id = decodeURIComponent(id)
      } catch {
        // Malformed percent escapes remain literal anchor text.
      }
      const target =
        root.querySelector<HTMLElement>(`[id="${CSS.escape(id)}"]`) ??
        root.querySelector<HTMLElement>(`[id="${CSS.escape('user-content-' + id)}"]`)
      target?.scrollIntoView({ block: 'start' })
      return
    }

    let target = href
    if (filePath && !URL_SCHEME_RE.test(href) && !href.startsWith('//')) {
      const hashIndex = href.indexOf('#')
      const pathPart = hashIndex !== -1 ? href.slice(0, hashIndex) : href
      const hashPart = hashIndex !== -1 ? href.slice(hashIndex) : ''
      const resolved = resolveMarkdownLocalPath(filePath, pathPart)
      if (resolved) target = `${resolved}${hashPart}`
    }
    void openLink(target, folderPath)
  }
</script>

{#if error}
  <p role="alert" class="px-6 py-4 text-danger">Markdown preview failed: {error}</p>
{/if}
<!-- Delegated clicks also receive native keyboard activation from rendered links. -->
<!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
<div bind:this={root} class="markdown-body px-6 py-4" onclick={handleClick}>
  {#key rendered}
    {@html rendered?.html ?? ''}
  {/key}
</div>
