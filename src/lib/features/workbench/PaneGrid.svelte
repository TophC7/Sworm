<script lang="ts">
  import Pane from '$lib/features/workbench/Pane.svelte'
  import { ResizableHandle, ResizablePane, ResizablePaneGroup } from '$lib/components/ui/resizable'
  import { createPane, type PaneSlot, type PaneState, type Tab } from '$lib/features/workbench/model'
  import { getAllTabs, getPanes, getQuadLayout, getSplitMode } from '$lib/features/workbench/state.svelte'

  const PANE_CLASS = 'min-h-0 min-w-0 overflow-hidden'

  let {
    projectId,
    projectPath
  }: {
    projectId: string
    projectPath: string
  } = $props()

  let panes = $derived(getPanes(projectId))
  let splitMode = $derived(getSplitMode(projectId))
  let quadLayout = $derived(getQuadLayout(projectId))
  let allTabs = $derived(getAllTabs(projectId))

  // Single-pass slot lookup instead of 6 separate .find() scans
  let panesBySlot = $derived(Object.fromEntries(panes.map((p) => [p.slot, p])) as Partial<Record<PaneSlot, PaneState>>)
  let leftPane = $derived(panesBySlot['left'] ?? createPane('left'))
  let rightPane = $derived(panesBySlot['right'] ?? createPane('right'))
  let topLeft = $derived(panesBySlot['top-left'] ?? createPane('top-left'))
  let topRight = $derived(panesBySlot['top-right'] ?? createPane('top-right'))
  let bottomLeft = $derived(panesBySlot['bottom-left'] ?? createPane('bottom-left'))
  let bottomRight = $derived(panesBySlot['bottom-right'] ?? createPane('bottom-right'))

  function tabsForPane(pane: PaneState): Tab[] {
    return pane.tabs.map((id) => allTabs.find((t) => t.id === id)).filter((t): t is Tab => t !== undefined)
  }

  function renderPane(pane: PaneState) {
    return {
      pane,
      tabs: tabsForPane(pane)
    }
  }
</script>

<div class="flex min-h-0 flex-1 flex-col overflow-hidden">
  {#if splitMode === 'single'}
    {@const sole = panes[0] ?? createPane('sole')}
    <Pane pane={sole} tabs={tabsForPane(sole)} {projectId} {projectPath} />
  {:else if splitMode === 'horizontal'}
    <ResizablePaneGroup direction="horizontal" autoSaveId={`${projectId}-h`}>
      <ResizablePane class={PANE_CLASS} defaultSize={50} minSize={20}>
        <Pane pane={leftPane} tabs={tabsForPane(leftPane)} {projectId} {projectPath} />
      </ResizablePane>
      <ResizableHandle />
      <ResizablePane class={PANE_CLASS} defaultSize={50} minSize={20}>
        <Pane pane={rightPane} tabs={tabsForPane(rightPane)} {projectId} {projectPath} />
      </ResizablePane>
    </ResizablePaneGroup>
  {:else if splitMode === 'vertical'}
    <ResizablePaneGroup direction="vertical" autoSaveId={`${projectId}-v`}>
      <ResizablePane class={PANE_CLASS} defaultSize={50} minSize={20}>
        <Pane pane={leftPane} tabs={tabsForPane(leftPane)} {projectId} {projectPath} />
      </ResizablePane>
      <ResizableHandle />
      <ResizablePane class={PANE_CLASS} defaultSize={50} minSize={20}>
        <Pane pane={rightPane} tabs={tabsForPane(rightPane)} {projectId} {projectPath} />
      </ResizablePane>
    </ResizablePaneGroup>
  {:else if splitMode === 'quad'}
    {#if quadLayout === 'top'}
      <ResizablePaneGroup direction="vertical" autoSaveId={`${projectId}-quad-top-v`}>
        <ResizablePane class={PANE_CLASS} defaultSize={50} minSize={20}>
          <Pane {...renderPane(topLeft)} {projectId} {projectPath} />
        </ResizablePane>
        <ResizableHandle />
        <ResizablePane class={PANE_CLASS} defaultSize={50} minSize={20}>
          <ResizablePaneGroup direction="horizontal" autoSaveId={`${projectId}-quad-top-h`}>
            <ResizablePane class={PANE_CLASS} defaultSize={50} minSize={20}>
              <Pane {...renderPane(bottomLeft)} {projectId} {projectPath} />
            </ResizablePane>
            <ResizableHandle />
            <ResizablePane class={PANE_CLASS} defaultSize={50} minSize={20}>
              <Pane {...renderPane(bottomRight)} {projectId} {projectPath} />
            </ResizablePane>
          </ResizablePaneGroup>
        </ResizablePane>
      </ResizablePaneGroup>
    {:else if quadLayout === 'bottom'}
      <ResizablePaneGroup direction="vertical" autoSaveId={`${projectId}-quad-bottom-v`}>
        <ResizablePane class={PANE_CLASS} defaultSize={50} minSize={20}>
          <ResizablePaneGroup direction="horizontal" autoSaveId={`${projectId}-quad-bottom-h`}>
            <ResizablePane class={PANE_CLASS} defaultSize={50} minSize={20}>
              <Pane {...renderPane(topLeft)} {projectId} {projectPath} />
            </ResizablePane>
            <ResizableHandle />
            <ResizablePane class={PANE_CLASS} defaultSize={50} minSize={20}>
              <Pane {...renderPane(topRight)} {projectId} {projectPath} />
            </ResizablePane>
          </ResizablePaneGroup>
        </ResizablePane>
        <ResizableHandle />
        <ResizablePane class={PANE_CLASS} defaultSize={50} minSize={20}>
          <Pane {...renderPane(bottomLeft)} {projectId} {projectPath} />
        </ResizablePane>
      </ResizablePaneGroup>
    {:else if quadLayout === 'left'}
      <ResizablePaneGroup direction="horizontal" autoSaveId={`${projectId}-quad-left-h`}>
        <ResizablePane class={PANE_CLASS} defaultSize={50} minSize={20}>
          <Pane {...renderPane(topLeft)} {projectId} {projectPath} />
        </ResizablePane>
        <ResizableHandle />
        <ResizablePane class={PANE_CLASS} defaultSize={50} minSize={20}>
          <ResizablePaneGroup direction="vertical" autoSaveId={`${projectId}-quad-left-v`}>
            <ResizablePane class={PANE_CLASS} defaultSize={50} minSize={20}>
              <Pane {...renderPane(topRight)} {projectId} {projectPath} />
            </ResizablePane>
            <ResizableHandle />
            <ResizablePane class={PANE_CLASS} defaultSize={50} minSize={20}>
              <Pane {...renderPane(bottomRight)} {projectId} {projectPath} />
            </ResizablePane>
          </ResizablePaneGroup>
        </ResizablePane>
      </ResizablePaneGroup>
    {:else if quadLayout === 'right'}
      <ResizablePaneGroup direction="horizontal" autoSaveId={`${projectId}-quad-right-h`}>
        <ResizablePane class={PANE_CLASS} defaultSize={50} minSize={20}>
          <ResizablePaneGroup direction="vertical" autoSaveId={`${projectId}-quad-right-v`}>
            <ResizablePane class={PANE_CLASS} defaultSize={50} minSize={20}>
              <Pane {...renderPane(topLeft)} {projectId} {projectPath} />
            </ResizablePane>
            <ResizableHandle />
            <ResizablePane class={PANE_CLASS} defaultSize={50} minSize={20}>
              <Pane {...renderPane(bottomLeft)} {projectId} {projectPath} />
            </ResizablePane>
          </ResizablePaneGroup>
        </ResizablePane>
        <ResizableHandle />
        <ResizablePane class={PANE_CLASS} defaultSize={50} minSize={20}>
          <Pane {...renderPane(topRight)} {projectId} {projectPath} />
        </ResizablePane>
      </ResizablePaneGroup>
    {:else}
      <ResizablePaneGroup direction="vertical" autoSaveId={`${projectId}-quad-v`}>
        <ResizablePane class={PANE_CLASS} defaultSize={50} minSize={20}>
          <ResizablePaneGroup direction="horizontal" autoSaveId={`${projectId}-quad-ht`}>
            <ResizablePane class={PANE_CLASS} defaultSize={50} minSize={20}>
              <Pane {...renderPane(topLeft)} {projectId} {projectPath} />
            </ResizablePane>
            <ResizableHandle />
            <ResizablePane class={PANE_CLASS} defaultSize={50} minSize={20}>
              <Pane {...renderPane(topRight)} {projectId} {projectPath} />
            </ResizablePane>
          </ResizablePaneGroup>
        </ResizablePane>
        <ResizableHandle />
        <ResizablePane class={PANE_CLASS} defaultSize={50} minSize={20}>
          <ResizablePaneGroup direction="horizontal" autoSaveId={`${projectId}-quad-hb`}>
            <ResizablePane class={PANE_CLASS} defaultSize={50} minSize={20}>
              <Pane {...renderPane(bottomLeft)} {projectId} {projectPath} />
            </ResizablePane>
            <ResizableHandle />
            <ResizablePane class={PANE_CLASS} defaultSize={50} minSize={20}>
              <Pane {...renderPane(bottomRight)} {projectId} {projectPath} />
            </ResizablePane>
          </ResizablePaneGroup>
        </ResizablePane>
      </ResizablePaneGroup>
    {/if}
  {/if}
</div>
