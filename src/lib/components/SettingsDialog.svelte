<!--
  @component
  SettingsDialog — IDE-style preferences surface.

  Flat dark dialog: sidebar of icon+label rows on the left,
  main pane of section blocks separated by full-width hairlines
  on the right. No nested card chrome. Views are self-contained
  and debounce-autosave on any edit.
-->

<script lang="ts">
  import { IconButton } from '$lib/components/ui/button'
  import { DialogContent, DialogRoot, DialogTitle } from '$lib/components/ui/dialog'
  import { ScrollArea } from '$lib/components/ui/scroll-area'
  import { TooltipContent, TooltipRoot, TooltipTrigger } from '$lib/components/ui/tooltip'
  import {
    AppWindow,
    SaveIcon,
    ChevronRight,
    LoaderCircle,
    PackageIcon,
    SettingsIcon,
    X
  } from '$lib/icons/lucideExports'
  import { loadSettings } from '$lib/stores/settings.svelte'
  import { getVersion } from '@tauri-apps/api/app'
  import { onMount, type Component } from 'svelte'

  // Cache the version once per module load. The value is immutable for
  // the running binary, so repeat opens of the dialog reuse it instead
  // of re-hitting the Tauri IPC. A null result means we're running
  // outside Tauri (e.g. vite preview).
  const versionPromise: Promise<string | null> = getVersion().catch(() => null)
  import GeneralView from './settings/GeneralView.svelte'
  import NixView from './settings/NixView.svelte'
  import ProvidersView from './settings/ProvidersView.svelte'
  import WindowView from './settings/WindowView.svelte'

  let { open = false, onClose }: { open?: boolean; onClose: () => void } = $props()

  // NAV //

  type View = 'general' | 'providers' | 'window' | 'nix'

  // NOTE: Nix has no Lucide match, so it uses the project's nixos.svg
  // via a mask span. `icon: null` flags the special render branch.
  type NavItem = { id: View; label: string; icon: Component | null }
  const NAV: NavItem[] = [
    { id: 'general', label: 'General', icon: SettingsIcon },
    { id: 'providers', label: 'Providers', icon: PackageIcon },
    { id: 'window', label: 'Window', icon: AppWindow },
    { id: 'nix', label: 'Nix', icon: null }
  ]

  let active = $state<View>('providers')
  let activeLabel = $derived(NAV.find((n) => n.id === active)?.label ?? '')

  // SAVE STATUS //

  let pending = $state(0)
  let savedFlash = $state(false)
  let flashTimer: ReturnType<typeof setTimeout> | undefined

  function onSaving() {
    pending++
    savedFlash = false
    clearTimeout(flashTimer)
  }

  function onSaved() {
    pending = Math.max(0, pending - 1)
    if (pending === 0) {
      savedFlash = true
      clearTimeout(flashTimer)
      flashTimer = setTimeout(() => (savedFlash = false), 1800)
    }
  }

  // VERSION //

  let version = $state<string | null>(null)

  onMount(() => {
    void loadSettings()
    void versionPromise.then((v) => (version = v))
  })

  let saveTooltip = $derived(
    pending > 0 ? 'Saving changes…' : savedFlash ? 'All changes saved' : 'Changes autosave 400ms after you stop typing.'
  )
</script>

<DialogRoot
  bind:open
  onOpenChange={(v) => {
    if (!v) onClose()
  }}
>
  <DialogContent class="h-[min(640px,85vh)] max-w-[880px] overflow-hidden bg-ground p-0">
    <div class="flex h-full min-h-0">
      <!-- SIDEBAR //-->
      <aside class="flex w-[200px] shrink-0 flex-col border-r border-edge">
        <header class="flex h-12 shrink-0 items-center border-b border-edge bg-surface px-4">
          <DialogTitle class="text-md font-semibold text-bright">Settings</DialogTitle>
        </header>

        <nav class="flex flex-1 flex-col gap-0.5 px-2 py-2 text-sm">
          {#each NAV as item (item.id)}
            <button
              type="button"
              onclick={() => (active = item.id)}
              class="flex cursor-pointer items-center gap-2.5 rounded-lg px-2 py-1.5 text-left transition-colors
                     {active === item.id ? 'bg-accent/15 text-bright' : 'text-muted hover:bg-surface hover:text-fg'}"
            >
              {#if item.icon}
                {@const IconComp = item.icon}
                <IconComp size={14} class="shrink-0" />
              {:else}
                <span
                  class="h-[14px] w-[14px] shrink-0 bg-current"
                  style="-webkit-mask: url(/svg/nixos.svg) no-repeat center / contain; mask: url(/svg/nixos.svg) no-repeat center / contain;"
                  role="img"
                  aria-label="Nix"
                ></span>
              {/if}
              <span class="flex-1">{item.label}</span>
              {#if active === item.id}
                <ChevronRight size={12} class="text-accent" />
              {/if}
            </button>
          {/each}
        </nav>
      </aside>

      <!-- MAIN //-->
      <main class="flex min-w-0 flex-1 flex-col">
        <header class="flex h-12 shrink-0 items-center justify-between border-b border-edge bg-surface px-5">
          <h2 class="text-md font-semibold text-bright">{activeLabel}</h2>
          <IconButton tooltip="Close" onclick={onClose}>
            <X size={14} />
          </IconButton>
        </header>

        <ScrollArea class="flex-1">
          {#if active === 'general'}
            <GeneralView />
          {:else if active === 'providers'}
            <ProvidersView {onSaving} {onSaved} />
          {:else if active === 'window'}
            <WindowView />
          {:else if active === 'nix'}
            <NixView {onSaving} {onSaved} />
          {/if}
        </ScrollArea>

        <footer class="flex h-10 shrink-0 items-center justify-between border-t border-edge px-5 text-sm">
          <span class="font-mono text-xs text-subtle">
            {#if version}Sworm v{version}{/if}
          </span>

          <TooltipRoot>
            <TooltipTrigger
              class="flex items-center gap-1.5 rounded-md px-1.5 py-0.5 transition-all"
              aria-label={saveTooltip}
            >
              {#if pending > 0}
                <LoaderCircle size={12} class="animate-spin text-muted" />
              {:else if savedFlash}
                <SaveIcon size={12} class="text-success" />
              {:else}
                <SaveIcon size={12} class="text-subtle" />
              {/if}
            </TooltipTrigger>
            <TooltipContent side="top">{saveTooltip}</TooltipContent>
          </TooltipRoot>
        </footer>
      </main>
    </div>
  </DialogContent>
</DialogRoot>
