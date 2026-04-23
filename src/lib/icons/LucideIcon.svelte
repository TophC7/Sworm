<!--
  @component
  LucideIcon — renders any Lucide icon by its kebab-case name.

  Uses Vite's import.meta.glob to lazy-load one chunk per icon, so the
  initial bundle stays small even though any of the ~1500 icons can be
  requested. Invalid names render nothing (callers should gate with a
  fallback icon if display is required).

  @param name - kebab-case Lucide icon name (e.g. "play", "git-branch")
  @param size - pixel dimension, default 14
-->

<script lang="ts">
  import type { Component } from 'svelte'

  let {
    name,
    size = 14,
    class: className = ''
  }: {
    name: string
    size?: number
    class?: string
  } = $props()

  const loaders = import.meta.glob<{ default: Component }>('/node_modules/@lucide/svelte/dist/icons/*.svelte')

  let IconComponent = $state<Component | null>(null)

  $effect(() => {
    let cancelled = false
    const loader = loaders[`/node_modules/@lucide/svelte/dist/icons/${name}.svelte`]
    IconComponent = null
    if (!loader) return
    loader()
      .then((mod) => {
        if (!cancelled) IconComponent = mod.default
      })
      .catch(() => {})
    return () => {
      cancelled = true
    }
  })
</script>

{#if IconComponent}
  <IconComponent {size} class={className} />
{/if}
