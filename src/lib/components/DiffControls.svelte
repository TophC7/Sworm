<script lang="ts">
  import { DiffModeEnum } from '@git-diff-view/svelte'
  import { Button } from '$lib/components/ui/button'
  import { TabsRoot, TabsList, TabsTrigger } from '$lib/components/ui/tabs'

  let {
    mode = $bindable(DiffModeEnum.Split),
    wrap = $bindable(false),
    fontSize = $bindable(13)
  }: {
    mode: DiffModeEnum
    wrap: boolean
    fontSize: number
  } = $props()

  function adjustFontSize(delta: number) {
    fontSize = Math.max(10, Math.min(20, fontSize + delta))
  }
</script>

<div class="flex shrink-0 items-center gap-2">
  <TabsRoot
    value={mode === DiffModeEnum.Split ? 'split' : 'unified'}
    onValueChange={(v) => {
      mode = v === 'split' ? DiffModeEnum.Split : DiffModeEnum.Unified
    }}
  >
    <TabsList>
      <TabsTrigger value="split">Split</TabsTrigger>
      <TabsTrigger value="unified">Unified</TabsTrigger>
    </TabsList>
  </TabsRoot>

  <Button
    variant={wrap ? 'accent' : 'ghost'}
    size="xs"
    onclick={() => (wrap = !wrap)}
    aria-pressed={wrap}
    title="Toggle line wrapping"
  >
    Wrap
  </Button>

  <div class="flex items-center gap-1">
    <Button
      variant="ghost"
      size="xs"
      onclick={() => adjustFontSize(-1)}
      disabled={fontSize <= 10}
      title="Decrease font size"
    >
      A-
    </Button>
    <span class="min-w-[2.8rem] text-center text-[0.68rem] text-muted tabular-nums">{fontSize}px</span>
    <Button
      variant="ghost"
      size="xs"
      onclick={() => adjustFontSize(1)}
      disabled={fontSize >= 20}
      title="Increase font size"
    >
      A+
    </Button>
  </div>
</div>
