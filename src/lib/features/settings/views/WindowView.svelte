<!--
  @component
  WindowView — window chrome and external-open routing preferences.
-->

<script lang="ts" module>
  import type { GeneralSettings } from '$lib/types/backend'
  let pendingPatch: Partial<GeneralSettings> = {}
  let saveChain = Promise.resolve()
</script>

<script lang="ts">
  import { onMount } from 'svelte'
  import { backend } from '$lib/api/backend'
  import { Select } from '$lib/components/ui/input'
  import { Switch } from '$lib/components/ui/switch'
  import { getWindowControls, setWindowControls } from '$lib/features/app-shell/window-controls/state.svelte'
  import { notify } from '$lib/features/notifications/state.svelte'
  import { getErrorMessage } from '$lib/features/notifications/runNotifiedTask'
  import { getSettings, loadSettings, saveGeneralSettings } from '$lib/features/settings/state/settings.svelte'
  import type { ExternalFileOpenMode, ExternalFolderOpenMode } from '$lib/types/backend'

  let wc = $derived(getWindowControls())
  let settings = $derived(getSettings())

  function toggleSystemDecorations(useSystemDecorations: boolean) {
    setWindowControls({ useSystemDecorations })
  }

  function toggleButton(key: 'showMinimize' | 'showMaximize' | 'showClose', value: boolean) {
    setWindowControls({ [key]: value })
  }

  function saveRouting(patch: Partial<GeneralSettings>): void {
    pendingPatch = { ...pendingPatch, ...patch }
    saveChain = saveChain
      .then(async () => {
        const current = settings?.general
        if (!current) return
        const next = { ...current, ...pendingPatch }
        pendingPatch = {}
        await saveGeneralSettings(next)
      })
      .catch((e) => {
        notify.error('Failed to save window routing settings', getErrorMessage(e))
      })
  }

  onMount(() => {
    let disposed = false
    let unlisten: (() => void) | undefined
    if (!settings) void loadSettings()
    void backend.settings
      .onChanged(() => void loadSettings())
      .then((cleanup) => {
        if (disposed) cleanup()
        else unlisten = cleanup
      })
    return () => {
      disposed = true
      unlisten?.()
    }
  })
</script>

<section class="flex flex-col gap-3 border-b border-edge px-5 py-4">
  <h3 class="text-md font-semibold text-bright">External Open Routing</h3>

  <label class="flex items-center gap-4">
    <div class="min-w-0 flex-1">
      <span class="text-sm text-fg">Open Folders From External Applications</span>
      <p class="text-xs text-subtle">Choose which window handles folders opened from the OS or command line.</p>
    </div>
    <Select
      class="w-44 shrink-0 text-sm"
      value={settings?.general.external_folder_open_mode ?? 'new_window'}
      onchange={(event) =>
        void saveRouting({
          external_folder_open_mode: event.currentTarget.value as ExternalFolderOpenMode
        })}
    >
      <option value="new_window">New window</option>
      <option value="focused_window">Focused window</option>
    </Select>
  </label>

  <label class="flex items-center gap-4">
    <div class="min-w-0 flex-1">
      <span class="text-sm text-fg">Open Files From External Applications</span>
      <p class="text-xs text-subtle">
        Prefer a window already showing the file's folder, or choose another destination.
      </p>
    </div>
    <Select
      class="w-56 shrink-0 text-sm"
      value={settings?.general.external_file_open_mode ?? 'prefer_folder'}
      onchange={(event) =>
        void saveRouting({
          external_file_open_mode: event.currentTarget.value as ExternalFileOpenMode
        })}
    >
      <option value="prefer_folder">Prefer existing folder window</option>
      <option value="focused_window">Focused window</option>
      <option value="new_window">New window</option>
    </Select>
  </label>
</section>

<section class="flex flex-col gap-2 border-b border-edge px-5 py-4">
  <h3 class="text-md font-semibold text-bright">Title bar</h3>

  <label class="flex items-center gap-3 py-1">
    <div class="flex-1">
      <span class="text-sm text-fg">Use system window decorations</span>
      <p class="text-xs text-subtle">Revert to the OS-provided title bar and controls.</p>
    </div>
    <Switch checked={wc.useSystemDecorations} onCheckedChange={toggleSystemDecorations} />
  </label>
</section>

{#if !wc.useSystemDecorations}
  <section class="flex flex-col gap-1 px-5 py-4">
    <h3 class="text-md font-semibold text-bright">Buttons</h3>
    <p class="pb-2 text-xs text-subtle">Hide individual custom-chrome buttons.</p>

    <label class="flex items-center justify-between border-b border-edge py-2.5">
      <span class="text-sm text-fg">Minimize</span>
      <Switch checked={wc.showMinimize} onCheckedChange={(v) => toggleButton('showMinimize', v)} />
    </label>
    <label class="flex items-center justify-between border-b border-edge py-2.5">
      <span class="text-sm text-fg">Maximize</span>
      <Switch checked={wc.showMaximize} onCheckedChange={(v) => toggleButton('showMaximize', v)} />
    </label>
    <label class="flex items-center justify-between py-2.5">
      <span class="text-sm text-fg">Close</span>
      <Switch checked={wc.showClose} onCheckedChange={(v) => toggleButton('showClose', v)} />
    </label>
  </section>
{/if}
