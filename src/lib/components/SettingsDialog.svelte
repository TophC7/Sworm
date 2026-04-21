<!--
  @component
  SettingsDialog — IDE-style preferences surface.

  Flat dark dialog: sectioned sidebar navigation on the left,
  main pane of section blocks separated by full-width hairlines
  on the right. General app controls live in the first section,
  while each supported language gets its own dedicated view.
-->

<script lang="ts">
  import { getBuiltinSettingsPages, preloadBuiltinCatalog } from '$lib/builtins/catalog'
  import {
    BreadcrumbItem,
    BreadcrumbList,
    BreadcrumbPage,
    BreadcrumbRoot,
    BreadcrumbSeparator
  } from '$lib/components/ui/breadcrumb'
  import FileIcon from '$lib/icons/FileIcon.svelte'
  import { Button, IconButton } from '$lib/components/ui/button'
  import { DialogContent, DialogRoot, DialogTitle } from '$lib/components/ui/dialog'
  import { ScrollArea } from '$lib/components/ui/scroll-area'
  import { TooltipContent, TooltipRoot, TooltipTrigger } from '$lib/components/ui/tooltip'
  import {
    AppWindow,
    PaintbrushIcon,
    SaveIcon,
    ChevronRight,
    LoaderCircle,
    PackageIcon,
    SettingsIcon,
    X
  } from '$lib/icons/lucideExports'
  import { loadSettings } from '$lib/stores/settings.svelte'
  import type { BuiltinSettingsPage } from '$lib/types/backend'
  import { getVersion } from '@tauri-apps/api/app'
  import { onMount, type Component } from 'svelte'

  // Cache the version once per module load. The value is immutable for
  // the running binary, so repeat opens of the dialog reuse it instead
  // of re-hitting the Tauri IPC. A null result means we're running
  // outside Tauri (e.g. vite preview).
  const versionPromise: Promise<string | null> = getVersion().catch(() => null)
  import GeneralView from './settings/GeneralView.svelte'
  import JsonSettingsEditorView from './settings/JsonSettingsEditorView.svelte'
  import LanguageSettingsView from './settings/LanguageSettingsView.svelte'
  import NixView from './settings/NixView.svelte'
  import ProvidersView from './settings/ProvidersView.svelte'
  import WindowView from './settings/WindowView.svelte'
  import type { JsonSettingsEditorSession } from './settings/jsonSettings'

  let { open = false, onClose }: { open?: boolean; onClose: () => void } = $props()

  // NAV //

  type View = 'appearance' | 'providers' | 'window' | string
  type NavIcon = { kind: 'lucide'; icon: Component } | { kind: 'file'; filename: string } | { kind: 'mask' }
  type NavItem = { id: View; label: string; icon: NavIcon }
  type NavSection = { title: string; items: NavItem[] }

  const GENERAL_ITEMS: NavItem[] = [
    { id: 'appearance', label: 'Appearance', icon: { kind: 'lucide', icon: PaintbrushIcon } },
    { id: 'providers', label: 'Providers', icon: { kind: 'lucide', icon: PackageIcon } },
    { id: 'window', label: 'Window', icon: { kind: 'lucide', icon: AppWindow } }
  ]
  let languagePages = $state<BuiltinSettingsPage[]>(getBuiltinSettingsPages())

  let LANGUAGE_ITEMS = $derived(
    languagePages.map((definition): NavItem => ({
      id: definition.id,
      label: definition.label,
      icon: { kind: 'file', filename: definition.icon_filename }
    }))
  )
  let NAV_SECTIONS = $derived([
    { title: 'General', items: GENERAL_ITEMS },
    { title: 'Languages', items: LANGUAGE_ITEMS }
  ])
  let FLAT_NAV = $derived(NAV_SECTIONS.flatMap((section) => section.items))

  let active = $state<View>('appearance')
  let activeItem = $derived(FLAT_NAV.find((item) => item.id === active) ?? FLAT_NAV[0])
  let activeLabel = $derived(activeItem.label)
  let activeLanguagePage = $derived(languagePages.find((definition) => definition.id === active) ?? null)
  let editorSession = $state<JsonSettingsEditorSession | null>(null)

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
    void preloadBuiltinCatalog()
      .then((catalog) => {
        languagePages = catalog.settings.pages
      })
      .catch(() => {})
  })

  let saveTooltip = $derived(
    pending > 0 ? 'Saving changes…' : savedFlash ? 'All changes saved' : 'Changes autosave 400ms after you stop typing.'
  )

  function openJsonEditor(session: JsonSettingsEditorSession) {
    editorSession = session
  }

  function closeJsonEditor() {
    editorSession = null
  }
</script>

<DialogRoot
  bind:open
  onOpenChange={(v) => {
    if (!v) {
      closeJsonEditor()
      onClose()
    }
  }}
>
  <DialogContent class="h-full max-h-224 max-w-5xl overflow-hidden bg-ground p-0" onModalClose={onClose}>
    <div class="flex h-full min-h-0">
      <!-- SIDEBAR //-->
      <aside class="flex w-[200px] shrink-0 flex-col border-r border-edge">
        <header class="flex h-12 shrink-0 items-center border-b border-edge bg-surface px-4">
          <DialogTitle class="flex items-center gap-2 text-md font-semibold text-bright">
            <SettingsIcon size={14} class="text-accent" />
            <span>Settings</span>
          </DialogTitle>
        </header>

        <nav class="flex flex-1 flex-col overflow-y-auto px-2 py-2 text-sm">
          {#each NAV_SECTIONS as section (section.title)}
            <div class="px-2 pt-2 pb-1 text-xs font-medium tracking-wide text-subtle uppercase">
              {section.title}
            </div>

            {#each section.items as item (item.id)}
              <button
                type="button"
                onclick={() => {
                  closeJsonEditor()
                  active = item.id
                }}
                class="flex cursor-pointer items-center gap-2.5 rounded-lg px-2 py-1.5 text-left transition-colors
                       {active === item.id ? 'bg-accent/15 text-bright' : 'text-muted hover:bg-surface hover:text-fg'}"
              >
                {#if item.icon.kind === 'lucide'}
                  {@const IconComp = item.icon.icon}
                  <IconComp size={14} class="shrink-0" />
                {:else if item.icon.kind === 'file'}
                  <FileIcon filename={item.icon.filename} size={14} />
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
          {/each}
        </nav>
      </aside>

      <!-- MAIN //-->
      <main class="flex min-w-0 flex-1 flex-col">
        <header class="flex h-12 shrink-0 items-center justify-between border-b border-edge bg-surface px-5">
          {#if editorSession}
            <BreadcrumbRoot class="flex min-w-0 flex-1">
              <BreadcrumbList class="min-w-0">
                <BreadcrumbItem class="min-w-0 max-w-48">
                  <Button variant="ghost" size="sm" class="-ml-2 h-auto px-2 py-1 text-sm text-muted hover:text-bright" onclick={closeJsonEditor}>
                    {activeLabel}
                  </Button>
                </BreadcrumbItem>
                <BreadcrumbSeparator />
                <BreadcrumbItem class="min-w-0 flex-1">
                  <BreadcrumbPage class="text-md">{editorSession.title}</BreadcrumbPage>
                </BreadcrumbItem>
              </BreadcrumbList>
            </BreadcrumbRoot>
          {:else}
            <DialogTitle class="truncate text-md font-semibold text-bright">{activeLabel}</DialogTitle>
          {/if}
          <IconButton tooltip="Close" onclick={onClose}>
            <X size={14} />
          </IconButton>
        </header>

        {#if editorSession}
          {#key editorSession.id}
            <div class="min-h-0 flex-1">
              <JsonSettingsEditorView
                editorId={editorSession.id}
                schema={editorSession.schema}
                defaults={editorSession.defaults}
                value={editorSession.value}
                description={editorSession.description}
                onSave={async (nextValue) => {
                  const session = editorSession
                  if (!session) return
                  await session.onSave(nextValue)
                  closeJsonEditor()
                }}
              />
            </div>
          {/key}
        {:else}
          <ScrollArea class="flex-1">
            {#if active === 'appearance'}
              <GeneralView />
            {:else if active === 'providers'}
              <ProvidersView {onSaving} {onSaved} />
            {:else if active === 'window'}
              <WindowView />
            {:else if activeLanguagePage}
              {#key activeLanguagePage.id}
                {#if activeLanguagePage.kind === 'nix'}
                  <NixView definition={activeLanguagePage} onOpenJsonEditor={openJsonEditor} {onSaving} {onSaved} />
                {:else}
                  <LanguageSettingsView definition={activeLanguagePage} onOpenJsonEditor={openJsonEditor} {onSaving} {onSaved} />
                {/if}
              {/key}
            {/if}
          </ScrollArea>
        {/if}

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
