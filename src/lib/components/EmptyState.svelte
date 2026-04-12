<script lang="ts">
  import { backend } from '$lib/api/backend'
  import StageView from '$lib/components/StageView.svelte'
  import { BlurFade } from '$lib/components/ui/blur-fade'
  import { addProject, getProjects } from '$lib/stores/projects.svelte'
  import { openProject } from '$lib/stores/workspace.svelte'
  import Clock from '@lucide/svelte/icons/clock'
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
  <div class="w-full max-w-md mx-auto">
    <BlurFade delay={0.05} duration={0.5} direction="up" offset={10}>
      <div class="flex items-center gap-3 mb-1">
        <Worm size={30} strokeWidth={2} class="text-accent shrink-0 -mr-1.5" />
        <h1 class="text-2xl text-bright font-medium m-0">Sworm</h1>
      </div>
      <p class="text-[0.8rem] text-muted mb-8">Agentic Development Environment</p>
    </BlurFade>

    <BlurFade delay={0.15} duration={0.4} direction="up" offset={8}>
      <h2 class="text-[0.7rem] uppercase tracking-widest text-muted mb-3">Start</h2>
      <button
        class="flex items-center gap-2.5 w-full text-left text-[0.85rem] text-fg hover:text-bright bg-transparent border-none cursor-pointer py-1.5 px-0 transition-colors group"
        onclick={handleOpen}
      >
        <FolderOpen size={15} class="text-muted group-hover:text-accent transition-colors" />
        Open Repository
      </button>
    </BlurFade>

    {#if projects.length > 0}
      <BlurFade delay={0.25} duration={0.4} direction="up" offset={8}>
        <h2 class="text-[0.7rem] uppercase tracking-widest text-muted mb-3 mt-8 flex items-center gap-1.5">
          <Clock size={11} />
          Recent
        </h2>
        <ul class="list-none m-0 p-0 flex flex-col gap-0.5">
          {#each projects.slice(0, 8) as project (project.id)}
            <li>
              <button
                class="flex items-baseline gap-2 w-full text-left bg-transparent border-none cursor-pointer py-1.5 px-0 transition-colors group"
                onclick={() => openProject(project.id)}
              >
                <span class="text-[0.85rem] text-accent group-hover:text-bright transition-colors truncate">
                  {dirName(project.path)}
                </span>
                <span class="text-[0.7rem] text-subtle group-hover:text-muted transition-colors truncate">
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
