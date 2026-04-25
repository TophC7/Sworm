<!--
  @component
  DirtyDiffPeekHeader -- header/action bar for Monaco's inline dirty-diff peek.
-->

<script lang="ts">
  import { IconButton } from '$lib/components/ui/button'
  import { TooltipProvider } from '$lib/components/ui/tooltip'
  import { ArrowDown, ArrowUp, Minus, Plus, Undo2Icon, X } from '$lib/icons/lucideExports'

  type StageKind = 'stage' | 'unstage'

  let {
    title,
    detail,
    stageLabel,
    stageKind = 'stage',
    revertLabel = '',
    canStage = true,
    canRevert = true,
    onStage,
    onRevert,
    onPrevious,
    onNext,
    onClose
  }: {
    title: string
    detail: string
    stageLabel: string
    stageKind?: StageKind
    revertLabel?: string
    canStage?: boolean
    canRevert?: boolean
    onStage?: () => void | Promise<void>
    onRevert?: () => void | Promise<void>
    onPrevious?: () => void
    onNext?: () => void
    onClose: () => void
  } = $props()

  function run(handler?: () => void | Promise<void>) {
    return () => {
      const result = handler?.()
      if (result && typeof result.catch === 'function') {
        void result.catch((error) => console.warn('dirty-diff-action:', error))
      }
    }
  }
</script>

<TooltipProvider>
  <div class="flex h-8 items-center border-b border-edge bg-surface pr-1 pl-3 text-sm">
    <div class="min-w-0 flex-1 truncate">
      <span class="font-mono text-fg">{title}</span>
      <span class="ml-2 text-muted">{detail}</span>
    </div>
    <div class="flex shrink-0 items-center gap-0.5">
      <IconButton tooltip={stageLabel} ariaLabel={stageLabel} onclick={run(onStage)} disabled={!canStage}>
        {#if stageKind === 'unstage'}
          <Minus size={13} />
        {:else}
          <Plus size={13} />
        {/if}
      </IconButton>
      {#if revertLabel}
        <IconButton tooltip={revertLabel} ariaLabel={revertLabel} onclick={run(onRevert)} disabled={!canRevert}>
          <Undo2Icon size={13} />
        </IconButton>
      {/if}
      <IconButton tooltip="Previous change" ariaLabel="Previous change" onclick={run(onPrevious)}>
        <ArrowUp size={13} />
      </IconButton>
      <IconButton tooltip="Next change" ariaLabel="Next change" onclick={run(onNext)}>
        <ArrowDown size={13} />
      </IconButton>
      <IconButton tooltip="Close" ariaLabel="Close" onclick={run(onClose)}>
        <X size={13} />
      </IconButton>
    </div>
  </div>
</TooltipProvider>
