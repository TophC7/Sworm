<!--
  @component
  PathBar — Nautilus-style location bar for the browser's folder columns.
  With `hosts`, it starts at the host (this machine or a server; clicking it
  lists all hosts), then the place the path lives on that host (Home or a
  volume), then clickable crumbs. While `editing`, it is the browser's text
  entry; the browser owns what typing means.
-->

<script lang="ts">
  import { backend } from '$lib/api/backend'
  import { Input } from '$lib/components/ui/input'
  import { HardDriveIcon, HomeIcon, MonitorIcon, ServerIcon } from '$lib/icons/lucideExports'
  import type { PathRoot } from '$lib/types/backend'
  import { cn } from '$lib/utils/cn'
  import { isEqualOrParent, normalizeAbsolutePath, splitRemotePath } from '$lib/utils/paths'
  import { localHostLabel } from './state.svelte'

  let {
    path,
    hosts,
    editing,
    draft = $bindable(),
    input = $bindable(null),
    error,
    onnavigate,
    onhosts,
    onedit,
    oninput,
    onkeydown,
    onblur
  }: {
    /** Location shown; `null` is the hosts list. */
    path: string | null
    /** Whether a hosts level exists above each host's filesystem. */
    hosts: boolean
    editing: boolean
    draft: string
    input?: HTMLInputElement | null
    error: string | null
    /** A crumb was clicked. */
    onnavigate: (path: string) => void
    /** The host crumb was clicked. */
    onhosts: () => void
    /** Empty space in the bar was clicked to type a location. */
    onedit: () => void
    oninput: () => void
    onkeydown: (event: KeyboardEvent) => void
    onblur: () => void
  } = $props()

  let pathRoot = $state<PathRoot | null>(null)
  let server = $derived(path === null ? null : (splitRemotePath(path)?.server ?? null))

  let root = $derived.by(() => {
    if (path === null) return null
    // The previous root keeps labelling the path until the fresh lookup lands.
    if (pathRoot && isEqualOrParent(pathRoot.path, path)) {
      return { icon: pathRoot.kind === 'home' ? HomeIcon : HardDriveIcon, label: pathRoot.label, path: pathRoot.path }
    }
    return { icon: HardDriveIcon, label: '/', path: server ? `sworm://${server}/` : '/' }
  })

  let crumbs = $derived.by(() => {
    if (path === null || root === null) return []
    const base = normalizeAbsolutePath(root.path)
    let ancestor = base.replace(/\/$/, '')
    return normalizeAbsolutePath(path)
      .slice(base.length)
      .split('/')
      .filter(Boolean)
      .map((label) => {
        ancestor += `/${label}`
        return { label, path: ancestor }
      })
  })

  // Every host answers where its own Nautilus would start the path.
  $effect(() => {
    const target = path
    if (target === null) return
    let cancelled = false
    backend.folders
      .pathRoot(target)
      .then((next) => {
        if (!cancelled) pathRoot = next
      })
      // A vanished folder keeps the last root; the listing reports the error.
      .catch(() => {})
    return () => {
      cancelled = true
    }
  })

  function keepEndVisible(element: HTMLElement): void {
    void crumbs
    element.scrollLeft = element.scrollWidth
  }

  const crumbClass =
    'flex h-6 shrink-0 cursor-pointer items-center gap-1.5 rounded-md px-1.5 text-muted transition-colors hover:bg-raised hover:text-bright focus-visible:shadow-focus-ring focus-visible:outline-none'
</script>

{#snippet separator()}
  <span class="shrink-0 text-subtle" aria-hidden="true">/</span>
{/snippet}

<div class="flex min-w-0 flex-1 flex-col gap-1">
  {#if editing}
    <Input
      bind:ref={input}
      bind:value={draft}
      aria-label="Location"
      aria-invalid={error !== null}
      spellcheck={false}
      autocomplete="off"
      class={cn('h-8 rounded-lg py-1 font-mono text-sm', error && 'border-danger focus:border-danger')}
      {oninput}
      {onkeydown}
      {onblur}
    />
    {#if error}<div class="px-1 text-xs text-danger">{error}</div>{/if}
  {:else}
    <div class="flex h-8 min-w-0 items-center gap-0.5 rounded-lg border border-edge bg-surface px-1 text-sm">
      {#if path === null}
        <span class="flex shrink-0 items-center gap-1.5 px-1.5 font-medium text-bright" aria-current="location">
          <ServerIcon size={14} class="shrink-0" />
          Servers
        </span>
      {:else}
        {#if hosts}
          <button type="button" class={cn(crumbClass, 'font-medium')} title="All servers" onclick={onhosts}>
            {#if server}<ServerIcon size={14} class="shrink-0" />{:else}<MonitorIcon size={14} class="shrink-0" />{/if}
            {server ?? localHostLabel()}
          </button>
          {@render separator()}
        {/if}
        {#if root}
          <button
            type="button"
            class={cn(crumbClass, 'font-medium')}
            title={root.path}
            onclick={() => onnavigate(root.path)}
          >
            <root.icon size={14} class="shrink-0" />
            {root.label}
          </button>
        {/if}
        <div class="flex min-w-0 scrollbar-none items-center gap-0.5 overflow-x-auto" {@attach keepEndVisible}>
          {#each crumbs as crumb, index (crumb.path)}
            {@render separator()}
            {#if index === crumbs.length - 1}
              <span class="shrink-0 px-1.5 font-medium text-bright" aria-current="location">{crumb.label}</span>
            {:else}
              <button type="button" class={crumbClass} title={crumb.path} onclick={() => onnavigate(crumb.path)}>
                {crumb.label}
              </button>
            {/if}
          {/each}
        </div>
      {/if}
      <button
        type="button"
        tabindex="-1"
        aria-label="Edit location"
        class="h-full min-w-4 flex-1 cursor-text border-none bg-transparent"
        onclick={onedit}
      ></button>
    </div>
  {/if}
</div>
