<script lang="ts">
  import { getWindowControls } from '$lib/features/app-shell/window-controls/state.svelte'
  import { IconButton } from '$lib/components/ui/button'
  import { Maximize, Minimize, Minus, X } from '$lib/icons/lucideExports'
  import { getCurrentWindow } from '@tauri-apps/api/window'

  let config = $derived(getWindowControls())
  let maximized = $state(false)

  const appWindow = getCurrentWindow()

  $effect(() => {
    const unlisten = appWindow.onResized(async () => {
      maximized = await appWindow.isMaximized()
    })
    appWindow.isMaximized().then((v) => (maximized = v))
    return () => {
      unlisten.then((fn) => fn())
    }
  })
</script>

{#if !config.useSystemDecorations}
  <div class="flex shrink-0 items-center pr-1">
    {#if config.showMinimize}
      <IconButton size="md" tooltip="Minimize" onclick={() => appWindow.minimize()}>
        <Minus size={12} strokeWidth={2} />
      </IconButton>
    {/if}

    {#if config.showMaximize}
      <IconButton size="md" tooltip={maximized ? 'Restore' : 'Maximize'} onclick={() => appWindow.toggleMaximize()}>
        {#if maximized}
          <Minimize size={10} strokeWidth={2} />
        {:else}
          <Maximize size={10} strokeWidth={2} />
        {/if}
      </IconButton>
    {/if}

    {#if config.showClose}
      <IconButton size="md" tone="danger" tooltip="Close" onclick={() => appWindow.close()}>
        <X size={11} strokeWidth={2} />
      </IconButton>
    {/if}
  </div>
{/if}
