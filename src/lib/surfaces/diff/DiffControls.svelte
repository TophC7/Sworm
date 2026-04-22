<script lang="ts">
  /*
   * Global controls for the Monaco multi-file diff view.
   *
   * These bindings drive Svelte state on `DiffStack`, which in turn
   * forwards settings to the editor pool via `updateSettings`. That
   * means every live AND every warm-pooled editor reflects the new
   * value immediately — no per-row mediation required.
   *
   *   sideBySide = renderSideBySide (unified vs split)
   *   wrap       = wordWrap + diffWordWrap
   *   fontSize   = fontSize
   */
  import { Button, IconButton } from '$lib/components/ui/button'
  import { Separator } from '$lib/components/ui/separator'
  import { TabsList, TabsRoot, TabsTrigger } from '$lib/components/ui/tabs'
  import { ChevronsDownUp, ChevronsUpDown } from '$lib/icons/lucideExports'

  let {
    sideBySide = $bindable(true),
    wrap = $bindable(false),
    fontSize = $bindable(13),
    anyExpanded = false,
    onToggleAll
  }: {
    sideBySide: boolean
    wrap: boolean
    fontSize: number
    anyExpanded?: boolean
    onToggleAll?: () => void
  } = $props()

  function adjustFontSize(delta: number) {
    fontSize = Math.max(10, Math.min(20, fontSize + delta))
  }
</script>

<div class="flex shrink-0 items-center gap-1.5">
  <!-- Unified (stacked) vs Split (side-by-side) — maps 1:1 onto Monaco's
       `renderSideBySide` option. -->
  <TabsRoot
    value={sideBySide ? 'split' : 'unified'}
    onValueChange={(v) => {
      sideBySide = v === 'split'
    }}
  >
    <TabsList>
      <TabsTrigger value="unified">Unified</TabsTrigger>
      <TabsTrigger value="split">Split</TabsTrigger>
    </TabsList>
  </TabsRoot>

  <!-- Expand / collapse every file's row body. -->
  {#if onToggleAll}
    <IconButton tooltip={anyExpanded ? 'Collapse all files' : 'Expand all files'} onclick={onToggleAll}>
      {#if anyExpanded}
        <ChevronsDownUp size={14} />
      {:else}
        <ChevronsUpDown size={14} />
      {/if}
    </IconButton>
  {/if}

  <Separator orientation="vertical" class="mx-0.5 h-4" />

  <Button variant={wrap ? 'accent' : 'ghost'} size="xs" onclick={() => (wrap = !wrap)} aria-pressed={wrap}>Wrap</Button>

  <Separator orientation="vertical" class="mx-0.5 h-4" />

  <div class="flex items-center gap-1">
    <Button variant="ghost" size="xs" onclick={() => adjustFontSize(-1)} disabled={fontSize <= 10}>A-</Button>
    <span class="min-w-[2.8rem] text-center text-2xs text-muted tabular-nums">{fontSize}px</span>
    <Button variant="ghost" size="xs" onclick={() => adjustFontSize(1)} disabled={fontSize >= 20}>A+</Button>
  </div>
</div>
