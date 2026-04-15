<script lang="ts">
  import { getWindowControls } from '$lib/stores/ui.svelte'
  import { TooltipRoot, TooltipTrigger, TooltipContent } from '$lib/components/ui/tooltip'
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
      <TooltipRoot>
        <TooltipTrigger
          class="flex h-8 w-8 cursor-pointer items-center justify-center rounded-md border-none bg-transparent text-muted transition-colors hover:bg-raised/70 hover:text-fg"
          aria-label="Minimize"
          onclick={() => appWindow.minimize()}
        >
          <Minus size={12} strokeWidth={2} />
        </TooltipTrigger>
        <TooltipContent side="bottom">Minimize</TooltipContent>
      </TooltipRoot>
    {/if}

    {#if config.showMaximize}
      <TooltipRoot>
        <TooltipTrigger
          class="flex h-8 w-8 cursor-pointer items-center justify-center rounded-md border-none bg-transparent text-muted transition-colors hover:bg-raised/70 hover:text-fg"
          aria-label={maximized ? 'Restore' : 'Maximize'}
          onclick={() => appWindow.toggleMaximize()}
        >
          {#if maximized}
            <Minimize size={10} strokeWidth={2} />
          {:else}
            <Maximize size={10} strokeWidth={2} />
          {/if}
        </TooltipTrigger>
        <TooltipContent side="bottom">{maximized ? 'Restore' : 'Maximize'}</TooltipContent>
      </TooltipRoot>
    {/if}

    {#if config.showClose}
      <TooltipRoot>
        <TooltipTrigger
          class="flex h-8 w-8 cursor-pointer items-center justify-center rounded-md border-none bg-transparent text-muted transition-colors hover:bg-danger-bg hover:text-bright"
          aria-label="Close"
          onclick={() => appWindow.close()}
        >
          <X size={11} strokeWidth={2} />
        </TooltipTrigger>
        <TooltipContent side="bottom">Close</TooltipContent>
      </TooltipRoot>
    {/if}
  </div>
{/if}
