<!--
  @component
  WindowView — window chrome preferences.

  Backed entirely by localStorage (ui.svelte.ts). Toggling system
  decorations calls the Tauri window API immediately so the change
  is visible without restart.
-->

<script lang="ts">
  import { Switch } from '$lib/components/ui/switch'
  import { getWindowControls, setWindowControls } from '$lib/features/app-shell/window-controls/state.svelte'
  import { getCurrentWindow } from '@tauri-apps/api/window'

  let wc = $derived(getWindowControls())

  function toggleSystemDecorations(useSystem: boolean) {
    setWindowControls({ useSystemDecorations: useSystem })
    void getCurrentWindow().setDecorations(useSystem)
  }

  function toggleButton(key: 'showMinimize' | 'showMaximize' | 'showClose', value: boolean) {
    setWindowControls({ [key]: value })
  }
</script>

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
