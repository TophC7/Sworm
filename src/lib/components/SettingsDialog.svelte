<script lang="ts">
  import { Badge } from '$lib/components/ui/badge'
  import { Button, IconButton } from '$lib/components/ui/button'
  import { Checkbox } from '$lib/components/ui/checkbox'
  import { DialogContent, DialogRoot, DialogTitle } from '$lib/components/ui/dialog'
  import { Input } from '$lib/components/ui/input'
  import { ScrollArea } from '$lib/components/ui/scroll-area'
  import { X } from '$lib/icons/lucideExports'
  import { notify } from '$lib/stores/notifications.svelte'
  import { refreshProviders } from '$lib/stores/providers.svelte'
  import {
    getSettings,
    getSettingsLoading,
    loadSettings,
    saveGeneralSettings,
    saveProviderConfig
  } from '$lib/stores/settings.svelte'
  import { getWindowControls, setWindowControls } from '$lib/stores/ui.svelte'
  import { getErrorMessage } from '$lib/utils/notifiedTask'
  import { getCurrentWindow } from '@tauri-apps/api/window'
  import { onMount } from 'svelte'

  let { open = false, onClose }: { open?: boolean; onClose: () => void } = $props()

  let settings = $derived(getSettings())
  let loading = $derived(getSettingsLoading())

  let generalDraft = $state({
    theme: 'dark',
    terminal_font_family: 'Monocraft',
    terminal_font_size: 13,
    nix_eval_timeout_secs: 600
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
        terminal_font_size: Number(generalDraft.terminal_font_size),
        nix_eval_timeout_secs: Number(generalDraft.nix_eval_timeout_secs)
      })
      flashMessage = 'General settings saved.'
      notify.success('General settings saved')
    } catch (error) {
      notify.error('Save settings failed', getErrorMessage(error))
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
      notify.success('Provider settings saved', providerId)
    } catch (error) {
      notify.error(`Save ${providerId} settings failed`, getErrorMessage(error))
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
    <header class="flex shrink-0 items-center justify-between border-b border-edge px-5 py-3">
      <DialogTitle>Settings</DialogTitle>
      <IconButton tooltip="Close" onclick={onClose}>
        <X size={14} />
      </IconButton>
    </header>

    {#if flashMessage}
      <div
        role="status"
        class="shrink-0 border-b border-success/40 bg-success-bg px-5 py-2 text-sm text-success-bright"
      >
        {flashMessage}
      </div>
    {/if}

    <ScrollArea class="flex-1">
      <section class="border-b border-edge">
        <div class="px-5 py-2">
          <span class="text-2xs font-semibold tracking-wide text-muted uppercase"> Window Controls </span>
        </div>

        <div class="flex flex-col gap-2.5 px-5 pb-3">
          <label class="flex items-center gap-2 text-base text-fg">
            <Checkbox checked={wcConfig.useSystemDecorations} onCheckedChange={handleToggleSystemDecorations} />
            <span>Use system window decorations</span>
          </label>

          {#if !wcConfig.useSystemDecorations}
            <div class="flex items-center gap-4 pl-5 text-base text-muted">
              <label class="flex items-center gap-1.5">
                <Checkbox
                  checked={wcConfig.showMinimize}
                  onCheckedChange={(v) => handleToggleWcButton('showMinimize', v)}
                />
                <span>Minimize</span>
              </label>
              <label class="flex items-center gap-1.5">
                <Checkbox
                  checked={wcConfig.showMaximize}
                  onCheckedChange={(v) => handleToggleWcButton('showMaximize', v)}
                />
                <span>Maximize</span>
              </label>
              <label class="flex items-center gap-1.5">
                <Checkbox checked={wcConfig.showClose} onCheckedChange={(v) => handleToggleWcButton('showClose', v)} />
                <span>Close</span>
              </label>
            </div>
          {/if}
        </div>
      </section>

      {#if loading && !settings}
        <div class="px-5 py-8 text-sm text-muted">Loading settings&hellip;</div>
      {:else if settings}
        <section class="border-b border-edge">
          <div class="flex items-center justify-between px-5 py-2">
            <span class="text-2xs font-semibold tracking-wide text-muted uppercase"> Terminal </span>
            <Button size="sm" onclick={handleSaveGeneral} disabled={savingGeneral}>
              {savingGeneral ? 'Saving\u2026' : 'Save'}
            </Button>
          </div>

          <div class="flex flex-col gap-3 px-5 pb-3">
            <label class="flex items-center gap-3 text-base">
              <span class="w-28 shrink-0 text-muted">Font family</span>
              <Input class="flex-1" bind:value={generalDraft.terminal_font_family} />
            </label>
            <label class="flex items-center gap-3 text-base">
              <span class="w-28 shrink-0 text-muted">Font size</span>
              <Input class="w-20" type="number" min="10" max="24" bind:value={generalDraft.terminal_font_size} />
            </label>
          </div>
        </section>

        <section class="border-b border-edge">
          <div class="flex items-center justify-between px-5 py-2">
            <span class="text-2xs font-semibold tracking-wide text-muted uppercase">Nix</span>
            <Button size="sm" onclick={handleSaveGeneral} disabled={savingGeneral}>
              {savingGeneral ? 'Saving\u2026' : 'Save'}
            </Button>
          </div>

          <div class="flex flex-col gap-3 px-5 pb-3">
            <label class="flex items-center gap-3 text-base">
              <span class="w-28 shrink-0 text-muted">Eval timeout</span>
              <Input
                class="w-24"
                type="number"
                min="30"
                max="3600"
                step="30"
                bind:value={generalDraft.nix_eval_timeout_secs}
              />
              <span class="text-sm text-subtle">
                seconds. Cold stores need 300s+; first flake build may take minutes.
              </span>
            </label>
          </div>
        </section>

        <section>
          <div class="px-5 py-2">
            <span class="text-2xs font-semibold tracking-wide text-muted uppercase"> Providers </span>
          </div>

          {#each settings.providers as entry (entry.provider.id)}
            <article class="mx-4 mb-3 overflow-hidden rounded-lg border border-edge bg-overlay">
              <div class="flex items-center justify-between border-b border-edge px-3 py-2">
                <div class="flex min-w-0 items-center gap-2">
                  <span class="text-md font-semibold text-bright">{entry.provider.label}</span>
                  <Badge variant={statusVariant(entry.provider.status)}>
                    {entry.provider.status}
                  </Badge>
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
                <div class="text-sm text-muted">
                  <span class="font-mono text-xs">
                    {entry.provider.version ?? 'Version unavailable'}
                  </span>
                  {#if entry.provider.resolved_path}
                    <span class="font-mono text-xs text-subtle">
                      &middot; {entry.provider.resolved_path}
                    </span>
                  {/if}
                </div>

                {#if entry.provider.message}
                  <div class="text-sm text-warning-bright">{entry.provider.message}</div>
                {/if}

                {#if entry.provider.status === 'missing'}
                  <div class="font-mono text-sm text-muted">{entry.provider.install_hint}</div>
                {/if}

                <label class="flex items-center gap-2 text-base">
                  <Checkbox
                    checked={providerDrafts[entry.provider.id]?.enabled ?? true}
                    onCheckedChange={(v) => updateProviderDraft(entry.provider.id, 'enabled', v)}
                  />
                  <span class="text-fg">Enabled</span>
                </label>

                <label class="flex items-center gap-3 text-base">
                  <span class="w-28 shrink-0 text-muted">Binary path</span>
                  <Input
                    class="flex-1"
                    value={providerDrafts[entry.provider.id]?.binaryPath ?? ''}
                    oninput={(event: Event) =>
                      updateProviderDraft(
                        entry.provider.id,
                        'binaryPath',
                        (event.currentTarget as HTMLInputElement).value
                      )}
                    placeholder="Use detected CLI"
                  />
                </label>

                <label class="flex items-center gap-3 text-base">
                  <span class="w-28 shrink-0 text-muted">Extra args</span>
                  <Input
                    class="flex-1"
                    value={providerDrafts[entry.provider.id]?.extraArgs ?? ''}
                    oninput={(event: Event) =>
                      updateProviderDraft(
                        entry.provider.id,
                        'extraArgs',
                        (event.currentTarget as HTMLInputElement).value
                      )}
                    placeholder="--flag value"
                  />
                </label>
              </div>
            </article>
          {/each}
        </section>
      {:else}
        <div class="px-5 py-8 text-sm text-muted">No settings data available.</div>
      {/if}
    </ScrollArea>
  </DialogContent>
</DialogRoot>
