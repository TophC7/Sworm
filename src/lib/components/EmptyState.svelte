<script lang="ts">
  import { backend } from '$lib/api/backend'
  import StageView from '$lib/components/StageView.svelte'
  import { BlurFade } from '$lib/components/ui/blur-fade'
  import { addProject, getProjects } from '$lib/stores/projects.svelte'
  import { openProject } from '$lib/stores/workspace.svelte'
  import FolderOpen from '@lucide/svelte/icons/folder-open'
  import Worm from '@lucide/svelte/icons/worm'

  let projects = $derived([...getProjects()].sort((a, b) => b.updated_at.localeCompare(a.updated_at)))

  async function handleOpen() {
    try {
      const path = await backend.projects.selectDirectory()
      if (path) {
        const project = await addProject(path)
        openProject(project.id)
      }
    } catch (e) {
      alert(`Failed to add project: ${e}`)
    }
  }

  function dirName(path: string): string {
    return path.split('/').pop() ?? path
  }

  function parentPath(path: string): string {
    const parts = path.split('/')
    parts.pop()
    const parent = parts.join('/')
    const home = '/home/' + (parts[2] ?? '')
    return parent.startsWith(home) ? '~' + parent.slice(home.length) : parent
  }
</script>

<StageView>
  <div class="mx-auto w-full max-w-md">
    <BlurFade delay={0.05} duration={0.5} direction="up" offset={10}>
      <div class="mb-1 flex items-center gap-3">
        <Worm size={30} strokeWidth={2} class="-mr-1.5 shrink-0 text-accent" />
        <h1 class="m-0 text-2xl font-medium text-bright">Sworm</h1>
      </div>
      <p class="mb-8 text-[0.8rem] text-muted">Agentic Development Environment</p>
    </BlurFade>

    <BlurFade delay={0.15} duration={0.4} direction="up" offset={8}>
      <h2 class="mb-3 text-[0.7rem] tracking-widest text-muted uppercase">Start</h2>
      <button
        class="group flex w-full cursor-pointer items-center gap-2.5 border-none bg-transparent px-0 py-1.5 text-left text-[0.85rem] text-fg transition-colors hover:text-bright"
        onclick={handleOpen}
      >
        <FolderOpen size={15} class="text-muted transition-colors group-hover:text-accent" />
        Open Repository
      </button>
    </BlurFade>

    {#if projects.length > 0}
      <BlurFade delay={0.25} duration={0.4} direction="up" offset={8}>
        <h2 class="mt-8 mb-3 flex items-center gap-1.5 text-[0.7rem] tracking-widest text-muted uppercase">Recent</h2>
        <ul class="m-0 flex list-none flex-col gap-0.5 p-0">
          {#each projects.slice(0, 8) as project (project.id)}
            <li>
              <button
                class="group flex w-full cursor-pointer items-baseline gap-2 border-none bg-transparent px-0 py-1.5 text-left transition-colors"
                onclick={() => openProject(project.id)}
              >
                <span class="truncate text-[0.85rem] text-accent transition-colors group-hover:text-bright">
                  {dirName(project.path)}
                </span>
                <span class="truncate text-[0.7rem] text-subtle transition-colors group-hover:text-muted">
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
