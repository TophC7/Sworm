<!--
  @component
  FolderPathBar — Nautilus-style location bar for the folder switcher.
  Root segment is the place the path lives (remote server, Home, or the
  local volume), then clickable crumbs. While `editing`, it is the
  switcher's text entry; the switcher owns what typing means.
-->

<script lang="ts">
  import { backend } from '$lib/api/backend'
  import { Input } from '$lib/components/ui/input'
  import { HardDriveIcon, HomeIcon, ServerIcon } from '$lib/icons/lucideExports'
  import type { PathRoot } from '$lib/types/backend'
  import { cn } from '$lib/utils/cn'
  import { isEqualOrParent, normalizeAbsolutePath, splitRemotePath } from '$lib/utils/paths'

  let {
    path,
    editing,
    draft = $bindable(),
    input = $bindable(null),
    error,
    onnavigate,
    onedit,
    oninput,
    onkeydown,
    onblur
  }: {
    path: string
    editing: boolean
    draft: string
    input?: HTMLInputElement | null
    error: string | null
    /** A crumb was clicked. */
    onnavigate: (path: string) => void
    /** Empty space in the bar was clicked to type a location. */
    onedit: () => void
    oninput: () => void
    onkeydown: (event: KeyboardEvent) => void
    onblur: () => void
  } = $props()

  let localRoot = $state<PathRoot | null>(null)

  let root = $derived.by(() => {
    const remote = splitRemotePath(path)
    if (remote) return { icon: ServerIcon, label: remote.server, path: `sworm://${remote.server}/` }
    // The previous root keeps labelling the path until the fresh lookup lands.
    if (localRoot && isEqualOrParent(localRoot.path, path)) {
      return {
        icon: localRoot.kind === 'home' ? HomeIcon : HardDriveIcon,
        label: localRoot.label,
        path: localRoot.path
      }
    }
    return { icon: HardDriveIcon, label: '/', path: '/' }
  })

  let crumbs = $derived.by(() => {
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

  // Remote paths root at their server; local ones ask where Nautilus would start.
  $effect(() => {
    const target = path
    if (splitRemotePath(target)) return
    let cancelled = false
    backend.folders
      .pathRoot(target)
      .then((next) => {
        if (!cancelled) localRoot = next
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
</script>

<div class="flex min-w-0 flex-1 flex-col gap-1">
  {#if editing}
    <Input
      bind:ref={input}
      bind:value={draft}
      aria-label="Folder location"
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
      <button
        type="button"
        class="flex h-6 shrink-0 cursor-pointer items-center gap-1.5 rounded-md px-1.5 font-medium text-muted transition-colors hover:bg-raised hover:text-bright focus-visible:shadow-focus-ring focus-visible:outline-none"
        title={root.path}
        onclick={() => onnavigate(root.path)}
      >
        <root.icon size={14} class="shrink-0" />
        {root.label}
      </button>
      <div class="flex min-w-0 scrollbar-none items-center gap-0.5 overflow-x-auto" {@attach keepEndVisible}>
        {#each crumbs as crumb, index (crumb.path)}
          <span class="shrink-0 text-subtle" aria-hidden="true">/</span>
          {#if index === crumbs.length - 1}
            <span class="shrink-0 px-1.5 font-medium text-bright" aria-current="location">{crumb.label}</span>
          {:else}
            <button
              type="button"
              class="h-6 shrink-0 cursor-pointer rounded-md px-1.5 text-muted transition-colors hover:bg-raised hover:text-bright focus-visible:shadow-focus-ring focus-visible:outline-none"
              title={crumb.path}
              onclick={() => onnavigate(crumb.path)}
            >
              {crumb.label}
            </button>
          {/if}
        {/each}
      </div>
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
