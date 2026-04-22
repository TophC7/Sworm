<script lang="ts">
  import { backend } from '$lib/api/backend'
  import ActivityMap from '$lib/components/ActivityMap.svelte'
  import StageView from '$lib/components/StageView.svelte'
  import { BlurFade } from '$lib/components/ui/blur-fade'
  import { addProject, getProjects } from '$lib/stores/projects.svelte'
  import { notify } from '$lib/stores/notifications.svelte'
  import { getActiveProjectId, openProject } from '$lib/workbench/state.svelte'
  import { hideProjectPicker, isProjectPickerOverride } from '$lib/stores/ui.svelte'
  import { parentPath } from '$lib/utils/path'
  import { getErrorMessage } from '$lib/utils/notifiedTask'
  import { FolderOpen, Worm } from '$lib/icons/lucideExports'

  let projects = $derived([...getProjects()].sort((a, b) => b.updated_at.localeCompare(a.updated_at)))

  async function handleOpen() {
    try {
      const path = await backend.projects.selectDirectory()
      if (path) {
        const project = await addProject(path)
        openProject(project.id)
      }
    } catch (e) {
      notify.error('Open project failed', getErrorMessage(e))
    }
  }

  function dirName(path: string): string {
    return path.split('/').pop() ?? path
  }

  // Esc dismisses the picker only when it's an override on top of a
  // real active project. Without an active project there's nowhere to
  // go back to, so we let the key fall through.
  function handleKeydown(e: KeyboardEvent) {
    if (e.key !== 'Escape') return
    if (!isProjectPickerOverride()) return
    if (!getActiveProjectId()) return
    e.preventDefault()
    hideProjectPicker()
  }
</script>

<svelte:window onkeydown={handleKeydown} />

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
        onclick={handleOpen}
      >
        <FolderOpen size={15} class="text-muted transition-colors group-hover:text-accent" />
        Open Repository
      </button>
    </BlurFade>

    <ActivityMap />

    {#if projects.length > 0}
      <BlurFade delay={0.25} duration={0.4} direction="up" offset={8}>
        <h2 class="mt-8 mb-3 flex items-center gap-1.5 text-xs tracking-widest text-muted uppercase">Recent</h2>
        <ul class="m-0 flex list-none flex-col gap-0.5 p-0">
          {#each projects.slice(0, 8) as project (project.id)}
            <li>
              <button
                class="group flex w-full cursor-pointer items-baseline gap-2 rounded-sm border-none bg-transparent px-0 py-1.5 text-left transition-colors focus-visible:shadow-focus-ring focus-visible:outline-none"
                onclick={() => openProject(project.id)}
              >
                <span class="truncate text-md text-accent transition-colors group-hover:text-bright">
                  {dirName(project.path)}
                </span>
                <span class="truncate text-xs text-subtle transition-colors group-hover:text-muted">
                  {parentPath(project.path)}
                </span>
              </button>
            </li>
          {/each}
        </ul>
      </BlurFade>
    {/if}
  </div>
</StageView>
