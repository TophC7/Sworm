<script lang="ts">
  import DiscoveredProjectsPanel from '$lib/features/activity-map/DiscoveredProjectsPanel.svelte'
  import StageView from '$lib/components/layout/StageView.svelte'
  import { BlurFade } from '$lib/components/ui/blur-fade'
  import { openFolderPicker } from '$lib/features/app-actions/actions.svelte'
  import { getRecentFolders } from '$lib/features/folders/state.svelte'
  import { openFolder } from '$lib/features/workbench/state.svelte'
  import { basename, parentPath } from '$lib/utils/paths'
  import { FolderOpen, Worm } from '$lib/icons/lucideExports'

  let recent = $derived(getRecentFolders())
</script>

<StageView>
  <div class="mx-auto w-full max-w-md">
    <BlurFade delay={0.05} duration={0.5} direction="up" offset={10}>
      <div class="mb-1 flex items-center gap-3">
        <Worm size={30} strokeWidth={2} class="-mr-1.5 shrink-0 text-accent" />
        <h1 class="m-0 text-2xl font-medium text-bright">Sworm</h1>
      </div>
      <p class="mb-8 text-base text-muted">Agentic Development Environment</p>
    </BlurFade>

    <BlurFade delay={0.15} duration={0.4} direction="up" offset={8}>
      <h2 class="mb-3 text-xs tracking-widest text-muted uppercase">Start</h2>
      <button
        class="group flex w-full cursor-pointer items-center gap-2.5 rounded-sm border-none bg-transparent px-0 py-1.5 text-left text-md text-fg transition-colors hover:text-bright focus-visible:shadow-focus-ring focus-visible:outline-none"
        onclick={() => void openFolderPicker()}
      >
        <FolderOpen size={15} class="text-muted transition-colors group-hover:text-accent" />
        Open Folder
      </button>
    </BlurFade>

    <DiscoveredProjectsPanel />

    {#if recent.length > 0}
      <BlurFade delay={0.25} duration={0.4} direction="up" offset={8}>
        <h2 class="mt-8 mb-3 flex items-center gap-1.5 text-xs tracking-widest text-muted uppercase">Recent</h2>
        <ul class="m-0 flex list-none flex-col gap-0.5 p-0">
          {#each recent.slice(0, 8) as path (path)}
            <li>
              <button
                class="group flex w-full cursor-pointer items-baseline gap-2 rounded-sm border-none bg-transparent px-0 py-1.5 text-left transition-colors focus-visible:shadow-focus-ring focus-visible:outline-none"
                onclick={() => void openFolder(path)}
              >
                <span class="truncate text-md text-accent transition-colors group-hover:text-bright">
                  {basename(path)}
                </span>
                <span class="truncate text-xs text-subtle transition-colors group-hover:text-muted">
                  {parentPath(path)}
                </span>
              </button>
            </li>
          {/each}
        </ul>
      </BlurFade>
    {/if}
  </div>
</StageView>
