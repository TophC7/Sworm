<script lang="ts">
  import { DiffMode } from '$lib/diff/types'
  import { Button } from '$lib/components/ui/button'
  import { TooltipRoot, TooltipTrigger, TooltipContent } from '$lib/components/ui/tooltip'
  import { TabsRoot, TabsList, TabsTrigger } from '$lib/components/ui/tabs'

  let {
    mode = $bindable(DiffMode.Split),
    wrap = $bindable(false),
    fontSize = $bindable(13)
  }: {
    mode: DiffMode
    wrap: boolean
    fontSize: number
  } = $props()

  function adjustFontSize(delta: number) {
    fontSize = Math.max(10, Math.min(20, fontSize + delta))
  }
</script>

<div class="flex shrink-0 items-center gap-2">
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

  <TooltipRoot>
    <TooltipTrigger>
      {#snippet child({ props })}
        <Button
          variant={wrap ? 'accent' : 'ghost'}
          size="xs"
          onclick={() => (wrap = !wrap)}
          aria-pressed={wrap}
          {...props}
        >
          Wrap
        </Button>
      {/snippet}
    </TooltipTrigger>
    <TooltipContent>Toggle line wrapping</TooltipContent>
  </TooltipRoot>

  <div class="flex items-center gap-1">
    <TooltipRoot>
      <TooltipTrigger disabled={fontSize <= 10}>
        {#snippet child({ props })}
          <Button variant="ghost" size="xs" onclick={() => adjustFontSize(-1)} disabled={fontSize <= 10} {...props}>
            A-
          </Button>
        {/snippet}
      </TooltipTrigger>
      <TooltipContent>Decrease font size</TooltipContent>
    </TooltipRoot>
    <span class="min-w-[2.8rem] text-center text-[0.68rem] text-muted tabular-nums">{fontSize}px</span>
    <TooltipRoot>
      <TooltipTrigger disabled={fontSize >= 20}>
        {#snippet child({ props })}
          <Button variant="ghost" size="xs" onclick={() => adjustFontSize(1)} disabled={fontSize >= 20} {...props}>
            A+
          </Button>
        {/snippet}
      </TooltipTrigger>
      <TooltipContent>Increase font size</TooltipContent>
    </TooltipRoot>
  </div>
</div>
