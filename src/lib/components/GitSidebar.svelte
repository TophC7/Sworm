<script lang="ts">
  import GitGraph from '$lib/components/GitGraph.svelte'
  import GitFileTree from '$lib/components/GitFileTree.svelte'
  import { Badge } from '$lib/components/ui/badge'
  import { Button } from '$lib/components/ui/button'
  import { ResizableHandle, ResizablePane, ResizablePaneGroup } from '$lib/components/ui/resizable'
  import { Sidebar, SidebarContent, SidebarHeader, SidebarProvider, SidebarTrigger } from '$lib/components/ui/sidebar'
  import { InfoTooltip } from '$lib/components/ui/tooltip'
  import { isGitSidebarCollapsed, setGitSidebarCollapsed } from '$lib/stores/ui.svelte'
  import type { GitSummary } from '$lib/types/backend'
  import { PanelLeftClose } from '@lucide/svelte'
  import ArrowDown from '@lucide/svelte/icons/arrow-down'
  import ArrowUp from '@lucide/svelte/icons/arrow-up'
  import GitBranchIcon from '@lucide/svelte/icons/git-branch'
  import RotateCw from '@lucide/svelte/icons/rotate-cw'

  let {
    summary,
    projectPath,
    onRefresh,
    onFileClick,
    onPersistTab,
    onCommitFileClick,
    onViewAllChanges
  }: {
    summary: GitSummary | null
    projectPath: string
    onRefresh?: () => void
    onFileClick?: (filePath: string, staged: boolean) => void
    onPersistTab?: () => void
    onCommitFileClick?: (hash: string, shortHash: string, message: string, filePath: string) => void
    onViewAllChanges?: (staged: boolean) => void
  } = $props()

  let collapsed = $derived(isGitSidebarCollapsed())
</script>

<SidebarProvider open={!collapsed} onOpenChange={(open) => setGitSidebarCollapsed(!open)}>
  <Sidebar>
    <SidebarHeader>
      <div class="flex items-center gap-1.5">
        <span class="text-[0.7rem] font-semibold tracking-wide text-muted uppercase">Git</span>
        {#if onRefresh}
          <Button variant="ghost" size="icon-sm" onclick={onRefresh} title="Refresh">
            <RotateCw size={11} />
          </Button>
        {/if}
      </div>
      <div class="flex items-center gap-1">
        <InfoTooltip ariaLabel="Explain git status badges" contentClass="w-72">
          <div class="space-y-2">
            <p class="font-medium text-bright">Git badges in this panel</p>
            <div class="grid grid-cols-[auto_1fr] gap-x-2 gap-y-1">
              <span class="font-mono font-bold text-warning">M</span>
              <span>Modified content</span>
              <span class="font-mono font-bold text-success">U</span>
              <span>Untracked (new file, not yet staged)</span>
              <span class="font-mono font-bold text-danger">D</span>
              <span>File deleted</span>
              <span class="font-mono font-bold text-accent">R</span>
              <span>File renamed according to git</span>
              <span class="font-mono font-bold">
                <span class="text-success">+12</span>
                <span class="text-muted"> / </span>
                <span class="text-danger">-4</span>
              </span>
              <span
                >Net line change. <code>+</code> means more lines added than removed, <code>-</code> means more lines removed
                than added.</span
              >
            </div>
            <p class="text-muted">
              The same file can appear in both sections if it has staged changes and newer unstaged edits.
            </p>
          </div>
        </InfoTooltip>
        <SidebarTrigger>
          <PanelLeftClose size={12} />
        </SidebarTrigger>
      </div>
    </SidebarHeader>

    {#if !collapsed && summary?.branch}
      <div class="flex shrink-0 items-center gap-1.5 border-b border-edge px-2.5 py-1">
        <Badge variant="default" class="gap-1 font-mono text-[0.68rem] normal-case">
          <GitBranchIcon size={10} />
          {summary.branch}
        </Badge>
        {#if (summary.ahead ?? 0) > 0}
          <Badge variant="success" class="gap-0.5 text-[0.68rem]">
            <ArrowUp size={9} />{summary.ahead}
          </Badge>
        {/if}
        {#if (summary.behind ?? 0) > 0}
          <Badge variant="danger" class="gap-0.5 text-[0.68rem]">
            <ArrowDown size={9} />{summary.behind}
          </Badge>
        {/if}
      </div>
    {/if}

    <SidebarContent>
      {#if summary}
        <ResizablePaneGroup direction="vertical">
          <ResizablePane defaultSize={60} minSize={15}>
            <div class="h-full overflow-y-auto">
              <GitFileTree {summary} {projectPath} {onFileClick} {onPersistTab} {onViewAllChanges} />
            </div>
          </ResizablePane>
          <ResizableHandle />
          <ResizablePane defaultSize={40} minSize={15}>
            <div class="h-full overflow-y-auto">
              <GitGraph {projectPath} onFileClick={onCommitFileClick} {onPersistTab} />
            </div>
          </ResizablePane>
        </ResizablePaneGroup>
      {:else}
        <div class="px-2.5 py-3 text-[0.75rem] text-subtle">Loading git info&hellip;</div>
      {/if}
    </SidebarContent>
  </Sidebar>
</SidebarProvider>
