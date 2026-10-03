<script lang="ts">
  import {
    clearNix,
    detectNix,
    evaluateNix,
    getNixDetection,
    getNixStatus,
    isNixEvaluating,
    selectNixFile
  } from '$lib/features/settings/state/nix.svelte'
  import nixosUrl from '$lib/assets/nixos.svg?url'
  import MaskIcon from '$lib/icons/MaskIcon.svelte'
  import { loadProvidersForFolder } from '$lib/features/sessions/providers/state.svelte'
  import {
    DropdownMenuRoot,
    DropdownMenuTrigger,
    DropdownMenuContent,
    DropdownMenuItem,
    DropdownMenuSeparator
  } from '$lib/components/ui/dropdown-menu'
  import { statusChipVariants } from '$lib/components/ui/status-chip'
  import { LoaderCircle, Check, X, CircleAlert } from '$lib/icons/lucideExports'
  import { notify, dismissNotification } from '$lib/features/notifications/state.svelte'
  import { getErrorMessage } from '$lib/features/notifications/runNotifiedTask'

  let { folderPath }: { folderPath: string } = $props()
  let detection = $derived(getNixDetection(folderPath))
  let evaluatingNow = $derived(isNixEvaluating(folderPath))
  let status = $derived(getNixStatus(folderPath))

  // Fence detection and its follow-on work when the active folder changes or closes.
  let folderGeneration = 0
  $effect(() => {
    const path = folderPath
    const generation = ++folderGeneration
    void detectNix(path)
      .then((result) => {
        if (generation !== folderGeneration) return
        if (result.selected?.status === 'ready') {
          void loadProvidersForFolder(path)
        } else if (result.selected?.status === 'pending') {
          void handleEvaluate()
        }
      })
      .catch((error) => {
        if (generation === folderGeneration) notify.error('Nix detection failed', getErrorMessage(error))
      })
    return () => {
      folderGeneration++
    }
  })

  async function handleSelect(nixFile: string) {
    if (evaluatingNow) return
    const path = folderPath
    const generation = folderGeneration
    try {
      if (detection?.selected?.nix_file !== nixFile) {
        await selectNixFile(path, nixFile)
      }
      if (generation !== folderGeneration) return
      await handleEvaluate()
    } catch (error) {
      if (generation === folderGeneration) notify.error('Select Nix file failed', getErrorMessage(error))
    }
  }
  async function handleEvaluate() {
    const path = folderPath
    const generation = folderGeneration
    const notificationId = notify.loading('Evaluating Nix environment')
    try {
      const record = await evaluateNix(path)
      if (generation !== folderGeneration) {
        dismissNotification(notificationId)
        return
      }
      await loadProvidersForFolder(path, () => generation === folderGeneration)
      if (generation !== folderGeneration) {
        dismissNotification(notificationId)
        return
      }
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
      if (generation !== folderGeneration) {
        dismissNotification(notificationId)
        return
      }
      notify.update(notificationId, {
        title: 'Nix evaluation failed',
        description: getErrorMessage(error),
        tone: 'error',
        loading: false
      })
    }
  }

  async function handleClear() {
    const path = folderPath
    const generation = folderGeneration
    try {
      await clearNix(path)
      if (generation !== folderGeneration) return
      await loadProvidersForFolder(path, () => generation === folderGeneration)
      if (generation === folderGeneration) notify.success('Disabled Nix environment')
    } catch (error) {
      if (generation === folderGeneration) {
        notify.error('Disable Nix environment failed', getErrorMessage(error))
      }
    }
  }
</script>

{#if status}
  <DropdownMenuRoot>
    <DropdownMenuTrigger class={statusChipVariants({ class: status.tone })}>
      {#if evaluatingNow}
        <LoaderCircle size={10} class="animate-spin" />
      {:else}
        <MaskIcon src={nixosUrl} width={10} label="Nix" />
      {/if}
      {status.label}
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
              Disable Nix env
            </span>
          </DropdownMenuItem>
        {/if}
      {/if}
    </DropdownMenuContent>
  </DropdownMenuRoot>
{/if}
