<script lang="ts">
  import { onMount } from 'svelte'
  import { getActiveProject } from '$lib/stores/projects.svelte'
  import {
    detectNix,
    evaluateNix,
    getNixDetection,
    isNixEvaluating,
    selectNixFile,
    clearNix
  } from '$lib/stores/nix.svelte'
  import { loadProvidersForProject } from '$lib/stores/providers.svelte'
  import { Badge } from '$lib/components/ui/badge'
  import {
    DropdownMenuRoot,
    DropdownMenuTrigger,
    DropdownMenuContent,
    DropdownMenuItem,
    DropdownMenuSeparator
  } from '$lib/components/ui/dropdown-menu'
  import { LoaderCircle, Check, X, CircleAlert } from '$lib/icons/lucideExports'

  let project = $derived(getActiveProject())
  let detection = $derived(project ? getNixDetection(project.id) : undefined)
  let evaluatingNow = $derived(project ? isNixEvaluating(project.id) : false)
  let hasNixFiles = $derived(detection && detection.detected_files.length > 0)

  let mounted = false
  let lastProjectId: string | null = null

  onMount(() => {
    mounted = true
  })

  // Auto-detect when active project changes, and load project-specific
  // providers if a Nix env was previously evaluated
  $effect(() => {
    if (!mounted || !project) return
    if (project.id === lastProjectId) return
    lastProjectId = project.id
    void detectNix(project.id).then((det) => {
      if (det.selected?.status === 'ready') {
        void loadProvidersForProject(project!.id)
      }
    })
  })

  async function handleSelect(nixFile: string) {
    if (!project) return
    await selectNixFile(project.id, nixFile)
  }

  async function handleEvaluate() {
    if (!project) return
    await evaluateNix(project.id)
    // Re-detect providers with Nix-augmented PATH
    await loadProvidersForProject(project.id)
  }

  async function handleClear() {
    if (!project) return
    await clearNix(project.id)
    // Reload providers without Nix env
    await loadProvidersForProject(project.id)
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
        return detection.selected.nix_file
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
      class="flex items-center gap-1 text-[0.68rem] {statusColor()} cursor-pointer border-none bg-transparent p-0 transition-colors hover:text-bright"
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
            <div class="px-3 py-1.5 text-[0.72rem] text-danger">
              {detection.selected.error_message ?? 'Evaluation failed'}
            </div>
            <DropdownMenuItem onclick={handleEvaluate}>Retry</DropdownMenuItem>
          {:else if detection.selected.status === 'pending'}
            <DropdownMenuItem onclick={handleEvaluate}>Evaluate</DropdownMenuItem>
          {:else if detection.selected.status === 'evaluating' || evaluatingNow}
            <div class="flex items-center gap-1.5 px-3 py-1.5 text-[0.72rem] text-muted">
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
