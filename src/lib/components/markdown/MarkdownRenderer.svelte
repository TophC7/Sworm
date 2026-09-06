<script lang="ts">
  import { renderMarkdown } from './renderMarkdown'
  import { resolveMarkdownLocalPath } from '$lib/utils/mediaAssets'
  import { openLink } from '$lib/features/workbench/links/openLink'
  import { getErrorMessage } from '$lib/features/notifications/runNotifiedTask'

  let {
    source,
    folderPath,
    filePath
  }: {
    source: string
    folderPath?: string
    filePath?: string | null
  } = $props()

  let html = $state('')
  let error = $state<string | null>(null)
  let root: HTMLDivElement

  $effect(() => {
    let current = true
    // Keep the previous complete preview until highlighting finishes.
    renderMarkdown(source, folderPath, filePath).then(
      (result) => {
        if (!current) return
        html = result
        error = null
      },
      (cause) => {
        if (current) error = getErrorMessage(cause)
      }
    )
    return () => {
      current = false
    }
  })

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
    if (filePath && !/^[a-zA-Z][a-zA-Z0-9+.-]*:/.test(href) && !href.startsWith('//')) {
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
<div bind:this={root} class="markdown-body px-6 py-4" onclick={handleClick}>{@html html}</div>
