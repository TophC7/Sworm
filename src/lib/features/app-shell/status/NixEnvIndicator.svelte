<script lang="ts">
  import { untrack } from 'svelte'
  import {
    clearNix,
    activateNixFolder,
    evaluateNix,
    getNixDetection,
    getNixStatus,
    isNixEvaluating,
    selectNixFile
  } from '$lib/features/settings/state/nix.svelte'
  import nixosUrl from '$lib/assets/nixos.svg?url'
  import MaskIcon from '$lib/icons/MaskIcon.svelte'
  import * as DropdownMenu from '$lib/components/ui/dropdown-menu'
  import { statusChipVariants } from '$lib/components/ui/status-chip'
  import { LoaderCircle, Check, X, CircleAlert } from '$lib/icons/lucideExports'

  let { folderPath }: { folderPath: string } = $props()
  let detection = $derived(getNixDetection(folderPath))
  let evaluatingNow = $derived(isNixEvaluating(folderPath))
  let status = $derived(getNixStatus(folderPath))

  $effect(() => {
    const path = folderPath
    untrack(() => void activateNixFolder(path))
  })
</script>

{#if status}
  <DropdownMenu.Root>
    <DropdownMenu.Trigger class={statusChipVariants({ class: status.tone })}>
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
    </DropdownMenu.Trigger>

    <DropdownMenu.Content class="min-w-[200px]" sideOffset={6}>
      {#if detection}
        {#each detection.detected_files as file}
          {@const isSelected = detection.selected?.nix_file === file}
          <DropdownMenu.Item class="flex items-center justify-between gap-2" onclick={() => selectNixFile(folderPath, file)}>
            <span class={isSelected ? 'text-accent' : ''}>{file}</span>
            {#if isSelected}
              <Check size={12} class="text-accent" />
            {/if}
          </DropdownMenu.Item>
        {/each}

        {#if detection.selected}
          <DropdownMenu.Separator />
          {#if detection.selected.status === 'ready'}
            <DropdownMenu.Item onclick={() => evaluateNix(folderPath)}>Re-evaluate</DropdownMenu.Item>
          {:else if detection.selected.status === 'error' || detection.selected.status === 'timeout'}
            <div class="px-3 py-1.5 text-sm text-danger">
              {detection.selected.error_message ?? 'Evaluation failed'}
            </div>
            <DropdownMenu.Item onclick={() => evaluateNix(folderPath)}>Retry</DropdownMenu.Item>
          {:else if detection.selected.status === 'pending'}
            <DropdownMenu.Item onclick={() => evaluateNix(folderPath)}>Evaluate</DropdownMenu.Item>
          {:else if detection.selected.status === 'evaluating' || evaluatingNow}
            <div class="flex items-center gap-1.5 px-3 py-1.5 text-sm text-muted">
              <LoaderCircle size={10} class="animate-spin" />
              Evaluating...
            </div>
          {/if}

          <DropdownMenu.Item destructive onclick={() => clearNix(folderPath)}>
            <span class="flex items-center gap-1.5">
              <X size={12} />
              Disable Nix env
            </span>
          </DropdownMenu.Item>
        {/if}
      {/if}
    </DropdownMenu.Content>
  </DropdownMenu.Root>
{/if}
