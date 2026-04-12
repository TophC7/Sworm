<script lang="ts">
  import { onMount } from 'svelte'
  import {
    loadSettings,
    getSettings,
    getSettingsLoading,
    saveGeneralSettings,
    saveProviderConfig
  } from '$lib/stores/settings.svelte'
  import { refreshProviders } from '$lib/stores/providers.svelte'
  import { getCurrentWindow } from '@tauri-apps/api/window'
  import { getWindowControls, setWindowControls } from '$lib/stores/ui.svelte'
  import { DialogRoot, DialogContent, DialogTitle } from '$lib/components/ui/dialog'
  import { Button } from '$lib/components/ui/button'
  import { Badge } from '$lib/components/ui/badge'
  import { ScrollArea } from '$lib/components/ui/scroll-area'
  import { Separator } from '$lib/components/ui/separator'
  import X from '@lucide/svelte/icons/x'

  let { open = false, onClose }: { open?: boolean; onClose: () => void } = $props()

  let settings = $derived(getSettings())
  let loading = $derived(getSettingsLoading())

  let generalDraft = $state({
    theme: 'dark',
    terminal_font_family: 'Monocraft',
    terminal_font_size: 13
  })
  let providerDrafts = $state<Record<string, { enabled: boolean; binaryPath: string; extraArgs: string }>>({})
  let savingGeneral = $state(false)
  let savingProviders = $state<Record<string, boolean>>({})
  let flashMessage = $state<string | null>(null)

  onMount(() => {
    void loadSettings()
  })

  $effect(() => {
    if (!settings) return

    generalDraft = { ...settings.general }
    providerDrafts = Object.fromEntries(
      settings.providers.map((entry) => [
        entry.provider.id,
        {
          enabled: entry.config.enabled,
          binaryPath: entry.config.binary_path_override ?? '',
          extraArgs: entry.config.extra_args.join(' ')
        }
      ])
    )
  })

  async function handleSaveGeneral() {
    savingGeneral = true
    flashMessage = null
    try {
      await saveGeneralSettings({
        theme: generalDraft.theme,
        terminal_font_family: generalDraft.terminal_font_family,
        terminal_font_size: Number(generalDraft.terminal_font_size)
      })
      flashMessage = 'General settings saved.'
    } finally {
      savingGeneral = false
    }
  }

  async function handleSaveProvider(providerId: string) {
    const draft = providerDrafts[providerId]
    if (!draft) return

    savingProviders = { ...savingProviders, [providerId]: true }
    flashMessage = null

    try {
      await saveProviderConfig({
        provider_id: providerId,
        enabled: draft.enabled,
        binary_path_override: draft.binaryPath.trim() || null,
        extra_args: draft.extraArgs
          .split(/\s+/)
          .map((v) => v.trim())
          .filter(Boolean)
      })
      await loadSettings()
      await refreshProviders()
      flashMessage = `Saved ${providerId} settings.`
    } finally {
      savingProviders = { ...savingProviders, [providerId]: false }
    }
  }

  function updateProviderDraft(
    providerId: string,
    key: 'enabled' | 'binaryPath' | 'extraArgs',
    value: boolean | string
  ) {
    providerDrafts = {
      ...providerDrafts,
      [providerId]: { ...providerDrafts[providerId], [key]: value }
    }
  }

  let wcConfig = $derived(getWindowControls())

  function handleToggleSystemDecorations(useSystem: boolean) {
    setWindowControls({ useSystemDecorations: useSystem })
    const win = getCurrentWindow()
    win.setDecorations(useSystem)
  }

  function handleToggleWcButton(key: 'showMinimize' | 'showMaximize' | 'showClose', value: boolean) {
    setWindowControls({ [key]: value })
  }

  function statusVariant(status: string): 'success' | 'warning' | 'danger' | 'default' {
    if (status === 'connected') return 'success'
    if (status === 'missing') return 'warning'
    if (status === 'error') return 'danger'
    return 'default'
  }
</script>

<DialogRoot
  bind:open
  onOpenChange={(v) => {
    if (!v) onClose()
  }}
>
  <DialogContent class="flex max-h-[80vh] max-w-lg flex-col p-0">
    <!-- Header -->
    <div class="flex shrink-0 items-center justify-between border-b border-edge px-5 py-3">
      <DialogTitle>Settings</DialogTitle>
      <Button variant="ghost" size="icon-sm" onclick={onClose}>
        <X size={14} />
      </Button>
    </div>

    {#if flashMessage}
      <div class="shrink-0 border-b border-edge bg-success/5 px-5 py-2 text-[0.82rem] text-success">
        {flashMessage}
      </div>
    {/if}

    <!-- Content -->
    <ScrollArea class="flex-1">
      <!-- Window controls (always visible, not backend-dependent) -->
      <div class="border-b border-edge">
        <div class="px-5 py-2">
          <span class="text-[0.7rem] font-semibold tracking-wide text-muted uppercase">Window Controls</span>
        </div>

        <div class="flex flex-col gap-2.5 px-5 pb-3">
          <label class="flex items-center gap-2 text-[0.82rem]">
            <input
              type="checkbox"
              checked={wcConfig.useSystemDecorations}
              onchange={(e) => handleToggleSystemDecorations((e.currentTarget as HTMLInputElement).checked)}
            />
            <span class="text-fg">Use system window decorations</span>
          </label>

          {#if !wcConfig.useSystemDecorations}
            <div class="flex items-center gap-4 pl-5 text-[0.82rem]">
              <label class="flex items-center gap-1.5">
                <input
                  type="checkbox"
                  checked={wcConfig.showMinimize}
                  onchange={(e) => handleToggleWcButton('showMinimize', (e.currentTarget as HTMLInputElement).checked)}
                />
                <span class="text-muted">Minimize</span>
              </label>
              <label class="flex items-center gap-1.5">
                <input
                  type="checkbox"
                  checked={wcConfig.showMaximize}
                  onchange={(e) => handleToggleWcButton('showMaximize', (e.currentTarget as HTMLInputElement).checked)}
                />
                <span class="text-muted">Maximize</span>
              </label>
              <label class="flex items-center gap-1.5">
                <input
                  type="checkbox"
                  checked={wcConfig.showClose}
                  onchange={(e) => handleToggleWcButton('showClose', (e.currentTarget as HTMLInputElement).checked)}
                />
                <span class="text-muted">Close</span>
              </label>
            </div>
          {/if}
        </div>
      </div>

      {#if loading && !settings}
        <div class="px-5 py-8 text-[0.82rem] text-muted">Loading settings&hellip;</div>
      {:else if settings}
        <!-- Terminal settings -->
        <div class="border-b border-edge">
          <div class="flex items-center justify-between px-5 py-2">
            <span class="text-[0.7rem] font-semibold tracking-wide text-muted uppercase">Terminal</span>
            <Button size="sm" onclick={handleSaveGeneral} disabled={savingGeneral}>
              {savingGeneral ? 'Saving\u2026' : 'Save'}
            </Button>
          </div>

          <div class="flex flex-col gap-3 px-5 pb-3">
            <label class="flex items-center gap-3 text-[0.82rem]">
              <span class="w-28 shrink-0 text-muted">Font family</span>
              <input class="field-input flex-1" bind:value={generalDraft.terminal_font_family} />
            </label>
            <label class="flex items-center gap-3 text-[0.82rem]">
              <span class="w-28 shrink-0 text-muted">Font size</span>
              <input
                class="field-input w-20"
                type="number"
                min="10"
                max="24"
                bind:value={generalDraft.terminal_font_size}
              />
            </label>
          </div>
        </div>

        <!-- Provider settings -->
        <div>
          <div class="px-5 py-2">
            <span class="text-[0.7rem] font-semibold tracking-wide text-muted uppercase">Providers</span>
          </div>

          {#each settings.providers as entry (entry.provider.id)}
            <div class="mx-4 mb-3 overflow-hidden rounded-lg border border-edge">
              <div class="flex items-center justify-between bg-surface px-3 py-2">
                <div class="flex min-w-0 items-center gap-2">
                  <span class="text-[0.85rem] font-semibold text-bright">{entry.provider.label}</span>
                  <Badge variant={statusVariant(entry.provider.status)}>{entry.provider.status}</Badge>
                </div>
                <Button
                  size="sm"
                  onclick={() => handleSaveProvider(entry.provider.id)}
                  disabled={savingProviders[entry.provider.id]}
                >
                  {savingProviders[entry.provider.id] ? 'Saving\u2026' : 'Save'}
                </Button>
              </div>

              <div class="flex flex-col gap-2.5 px-3 py-2.5">
                <div class="text-[0.75rem] text-muted">
                  {entry.provider.version ?? 'Version unavailable'}
                  {#if entry.provider.resolved_path}
                    <span class="text-subtle"> &middot; {entry.provider.resolved_path}</span>
                  {/if}
                </div>

                {#if entry.provider.message}
                  <div class="text-[0.75rem] text-warning">{entry.provider.message}</div>
                {/if}

                {#if entry.provider.status === 'missing'}
                  <div class="font-mono text-[0.75rem] text-muted">{entry.provider.install_hint}</div>
                {/if}

                <label class="flex items-center gap-2 text-[0.82rem]">
                  <input
                    type="checkbox"
                    checked={providerDrafts[entry.provider.id]?.enabled ?? true}
                    onchange={(event) =>
                      updateProviderDraft(
                        entry.provider.id,
                        'enabled',
                        (event.currentTarget as HTMLInputElement).checked
                      )}
                  />
                  <span class="text-fg">Enabled</span>
                </label>

                <label class="flex items-center gap-3 text-[0.82rem]">
                  <span class="w-28 shrink-0 text-muted">Binary path</span>
                  <input
                    class="field-input flex-1"
                    value={providerDrafts[entry.provider.id]?.binaryPath ?? ''}
                    oninput={(event) =>
                      updateProviderDraft(
                        entry.provider.id,
                        'binaryPath',
                        (event.currentTarget as HTMLInputElement).value
                      )}
                    placeholder="Use detected CLI"
                  />
                </label>

                <label class="flex items-center gap-3 text-[0.82rem]">
                  <span class="w-28 shrink-0 text-muted">Extra args</span>
                  <input
                    class="field-input flex-1"
                    value={providerDrafts[entry.provider.id]?.extraArgs ?? ''}
                    oninput={(event) =>
                      updateProviderDraft(
                        entry.provider.id,
                        'extraArgs',
                        (event.currentTarget as HTMLInputElement).value
                      )}
                    placeholder="--flag value"
                  />
                </label>
              </div>
            </div>
          {/each}
        </div>
      {:else}
        <div class="px-5 py-8 text-[0.82rem] text-muted">No settings data available.</div>
      {/if}
    </ScrollArea>
  </DialogContent>
</DialogRoot>
