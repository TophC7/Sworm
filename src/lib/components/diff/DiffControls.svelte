<script lang="ts">
  import { Button, IconButton } from '$lib/components/ui/button'
  import { Separator } from '$lib/components/ui/separator'
  import { TabsList, TabsRoot, TabsTrigger } from '$lib/components/ui/tabs'
  import { DiffMode } from '$lib/diff/types'
  import { ChevronsDownUp, ChevronsUpDown } from '$lib/icons/lucideExports'

  let {
    mode = $bindable(DiffMode.Split),
    wrap = $bindable(false),
    fontSize = $bindable(13),
    allExpanded = false,
    onToggleAll
  }: {
    mode: DiffMode
    wrap: boolean
    fontSize: number
    allExpanded?: boolean
    onToggleAll?: () => void
  } = $props()

  function adjustFontSize(delta: number) {
    fontSize = Math.max(10, Math.min(20, fontSize + delta))
  }
</script>

<div class="flex shrink-0 items-center gap-1.5">
  <!-- Split / Unified -->
  <TabsRoot
    value={mode === DiffMode.Split ? 'split' : 'unified'}
    onValueChange={(v) => {
      mode = v === 'split' ? DiffMode.Split : DiffMode.Unified
    }}
  >
    <TabsList>
      <TabsTrigger value="split">Split</TabsTrigger>
      <TabsTrigger value="unified">Unified</TabsTrigger>
    </TabsList>
  </TabsRoot>

  <!-- Expand / collapse all files. Arrow direction reflects the action the
       next click will perform: arrows-outward means "expand", arrows-inward
       means "collapse". -->
  {#if onToggleAll}
    <IconButton tooltip={allExpanded ? 'Collapse all files' : 'Expand all files'} onclick={onToggleAll}>
      {#if allExpanded}
        <ChevronsDownUp size={14} />
      {:else}
        <ChevronsUpDown size={14} />
      {/if}
    </IconButton>
  {/if}

  <Separator orientation="vertical" class="mx-0.5 h-4" />

  <!-- Wrap toggle -->
  <Button variant={wrap ? 'accent' : 'ghost'} size="xs" onclick={() => (wrap = !wrap)} aria-pressed={wrap}>Wrap</Button>

  <Separator orientation="vertical" class="mx-0.5 h-4" />

  <!-- Font controls -->
  <div class="flex items-center gap-1">
    <Button variant="ghost" size="xs" onclick={() => adjustFontSize(-1)} disabled={fontSize <= 10}>A-</Button>
    <span class="min-w-[2.8rem] text-center text-2xs text-muted tabular-nums">{fontSize}px</span>
    <Button variant="ghost" size="xs" onclick={() => adjustFontSize(1)} disabled={fontSize >= 20}>A+</Button>
  </div>
</div>
