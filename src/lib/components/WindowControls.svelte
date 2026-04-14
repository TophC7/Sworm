<script lang="ts">
  import { getWindowControls } from '$lib/stores/ui.svelte'
  import { Maximize, Minimize } from '@lucide/svelte'
  import { Minus, X } from '$lib/icons/lucideExports'
  import { getCurrentWindow } from '@tauri-apps/api/window'

  let config = $derived(getWindowControls())
  let maximized = $state(false)

  const appWindow = getCurrentWindow()

  // Track maximized state for visual feedback
  $effect(() => {
    const unlisten = appWindow.onResized(async () => {
      maximized = await appWindow.isMaximized()
    })
    // Check initial state
    appWindow.isMaximized().then((v) => (maximized = v))
    return () => {
      unlisten.then((fn) => fn())
    }
  })
</script>

{#if !config.useSystemDecorations}
  <div class="flex shrink-0 items-center pr-1">
    {#if config.showMinimize}
      <button
        class="flex h-8 w-8 cursor-pointer items-center justify-center rounded-md border-none bg-transparent text-muted transition-colors hover:bg-raised/70 hover:text-fg"
        onclick={() => appWindow.minimize()}
        title="Minimize"
      >
        <Minus size={12} strokeWidth={2} />
      </button>
    {/if}

    {#if config.showMaximize}
      <button
        class="flex h-8 w-8 cursor-pointer items-center justify-center rounded-md border-none bg-transparent text-muted transition-colors hover:bg-raised/70 hover:text-fg"
        onclick={() => appWindow.toggleMaximize()}
        title={maximized ? 'Restore' : 'Maximize'}
      >
        {#if maximized}
          <Minimize size={10} strokeWidth={2} />
        {:else}
          <Maximize size={10} strokeWidth={2} />
        {/if}
      </button>
    {/if}

    {#if config.showClose}
      <button
        class="flex h-8 w-8 cursor-pointer items-center justify-center rounded-md border-none bg-transparent text-muted transition-colors hover:bg-danger-bg hover:text-bright"
        onclick={() => appWindow.close()}
        title="Close"
      >
        <X size={11} strokeWidth={2} />
      </button>
    {/if}
  </div>
{/if}
