<script lang="ts">
  import {
    clearNix,
    detectNix,
    evaluateNix,
    getNixDetection,
    isNixEvaluating,
    selectNixFile
  } from '$lib/features/settings/state/nix.svelte'
  import { loadProvidersForFolder } from '$lib/features/sessions/providers/state.svelte'
  import {
    DropdownMenuRoot,
    DropdownMenuTrigger,
    DropdownMenuContent,
    DropdownMenuItem,
    DropdownMenuSeparator
  } from '$lib/components/ui/dropdown-menu'
  import { LoaderCircle, Check, X, CircleAlert } from '$lib/icons/lucideExports'
  import { notify } from '$lib/features/notifications/state.svelte'
  import { getErrorMessage } from '$lib/features/notifications/runNotifiedTask'

  let { folderPath }: { folderPath: string } = $props()
  let detection = $derived(getNixDetection(folderPath))
  let evaluatingNow = $derived(isNixEvaluating(folderPath))
  let hasNixFiles = $derived(detection && detection.detected_files.length > 0)

  // Detect once per active folder and refresh provider availability when
  // an evaluated Nix environment contributes commands to PATH.
  $effect(() => {
    void detectNix(folderPath).then((result) => {
      if (result.selected?.status === 'ready') void loadProvidersForFolder(folderPath)
    })
  })

  async function handleSelect(nixFile: string) {
    try {
      await selectNixFile(folderPath, nixFile)
      notify.success('Selected Nix file', nixFile)
    } catch (error) {
      notify.error('Select Nix file failed', getErrorMessage(error))
    }
  }

  async function handleEvaluate() {
    const notificationId = notify.loading('Evaluating Nix environment')
    try {
      const record = await evaluateNix(folderPath)
      await loadProvidersForFolder(folderPath)
      if (record.status === 'ready') {
        notify.update(notificationId, {
          title: 'Nix environment ready',
          description: record.nix_file,
          tone: 'success',
          loading: false
        })
        return
      }
      notify.update(notificationId, {
        title: record.status === 'timeout' ? 'Nix evaluation timed out' : 'Nix evaluation failed',
        description: record.error_message ?? record.nix_file,
        tone: 'error',
        loading: false
      })
    } catch (error) {
      notify.update(notificationId, {
        title: 'Nix evaluation failed',
        description: getErrorMessage(error),
        tone: 'error',
        loading: false
      })
    }
  }

  async function handleClear() {
    try {
      await clearNix(folderPath)
      await loadProvidersForFolder(folderPath)
      notify.success('Cleared Nix environment')
    } catch (error) {
      notify.error('Clear Nix environment failed', getErrorMessage(error))
    }
  }

  function statusColor(): string {
    if (evaluatingNow) return 'text-warning'
    if (!detection?.selected) return 'text-subtle'
    switch (detection.selected.status) {
      case 'ready':
        return 'text-success'
      case 'error':
      case 'timeout':
        return 'text-danger'
      default:
        return 'text-muted'
    }
  }

  function statusLabel(): string {
    if (evaluatingNow) return 'Evaluating...'
    if (!detection?.selected) return 'Nix available'
    switch (detection.selected.status) {
      case 'ready':
      case 'pending':
        return detection.selected.nix_file
      case 'evaluating':
        return 'Evaluating...'
      case 'error':
        return 'Error'
      case 'timeout':
        return 'Timeout'
      default:
        return detection.selected.nix_file
    }
  }
</script>

{#if hasNixFiles}
  <DropdownMenuRoot>
    <DropdownMenuTrigger
      class="flex cursor-pointer items-center gap-1 rounded-full border border-edge bg-raised px-2 py-0.5 text-xs {statusColor()} transition-colors hover:border-accent/50 hover:text-bright"
    >
      {#if evaluatingNow}
        <LoaderCircle size={10} class="animate-spin" />
      {:else}
        <span
          class="h-[10px] w-[10px] shrink-0 bg-current"
          style="-webkit-mask: url(/svg/nixos.svg) no-repeat center / contain; mask: url(/svg/nixos.svg) no-repeat center / contain;"
          role="img"
          aria-label="Nix"
        ></span>
      {/if}
      {statusLabel()}
      {#if detection?.selected?.status === 'ready'}
        <Check size={8} />
      {:else if detection?.selected?.status === 'error' || detection?.selected?.status === 'timeout'}
        <CircleAlert size={8} />
      {/if}
    </DropdownMenuTrigger>

    <DropdownMenuContent class="min-w-[200px]" sideOffset={6}>
      {#if detection}
        {#each detection.detected_files as file}
          {@const isSelected = detection.selected?.nix_file === file}
          <DropdownMenuItem class="flex items-center justify-between gap-2" onclick={() => handleSelect(file)}>
            <span class={isSelected ? 'text-accent' : ''}>{file}</span>
            {#if isSelected}
              <Check size={12} class="text-accent" />
            {/if}
          </DropdownMenuItem>
        {/each}

        {#if detection.selected}
          <DropdownMenuSeparator />
          {#if detection.selected.status === 'ready'}
            <DropdownMenuItem onclick={handleEvaluate}>Re-evaluate</DropdownMenuItem>
          {:else if detection.selected.status === 'error' || detection.selected.status === 'timeout'}
            <div class="px-3 py-1.5 text-sm text-danger">
              {detection.selected.error_message ?? 'Evaluation failed'}
            </div>
            <DropdownMenuItem onclick={handleEvaluate}>Retry</DropdownMenuItem>
          {:else if detection.selected.status === 'pending'}
            <DropdownMenuItem onclick={handleEvaluate}>Evaluate</DropdownMenuItem>
          {:else if detection.selected.status === 'evaluating' || evaluatingNow}
            <div class="flex items-center gap-1.5 px-3 py-1.5 text-sm text-muted">
              <LoaderCircle size={10} class="animate-spin" />
              Evaluating...
            </div>
          {/if}

          <DropdownMenuItem destructive onclick={handleClear}>
            <span class="flex items-center gap-1.5">
              <X size={12} />
              Clear Nix env
            </span>
          </DropdownMenuItem>
        {/if}
      {/if}
    </DropdownMenuContent>
  </DropdownMenuRoot>
{/if}
