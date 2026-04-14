<script lang="ts">
  import { Dialog } from 'bits-ui'
  import {
    Command,
    CommandEmpty,
    CommandGroup,
    CommandInput,
    CommandItem,
    CommandList,
    CommandSeparator,
    CommandShortcut
  } from '$lib/components/ui/command'
  import { cn } from '$lib/utils/cn'
  import { getProjects } from '$lib/stores/projects.svelte'
  import { closeProject, getActiveProjectId, getOpenProjectIds, openProject } from '$lib/stores/workspace.svelte'
  import { isGitSidebarCollapsed, toggleGitSidebar, zoomIn, zoomOut, zoomReset } from '$lib/stores/ui.svelte'

  import FolderOpenIcon from '@lucide/svelte/icons/folder-open'
  import FolderClockIcon from '@lucide/svelte/icons/folder-clock'
  import SettingsIcon from '@lucide/svelte/icons/settings'
  import PanelLeftIcon from '@lucide/svelte/icons/panel-left'
  import ZoomInIcon from '@lucide/svelte/icons/zoom-in'
  import ZoomOutIcon from '@lucide/svelte/icons/zoom-out'
  import RotateCcwIcon from '@lucide/svelte/icons/rotate-ccw'
  import XIcon from '@lucide/svelte/icons/x'

  let {
    onNewProject,
    onSettings
  }: {
    onNewProject: () => void
    onSettings: () => void
  } = $props()

  let open = $state(false)
  let search = $state('')

  let projects = $derived(getProjects())
  let openIds = $derived(getOpenProjectIds())
  let activeId = $derived(getActiveProjectId())
  let recentProjects = $derived(projects.filter((p) => !openIds.includes(p.id)))
  let sidebarCollapsed = $derived(isGitSidebarCollapsed())

  // Global keyboard shortcut: Ctrl+Shift+P
  $effect(() => {
    function onKeyDown(e: KeyboardEvent) {
      if (e.ctrlKey && e.shiftKey && e.key === 'P') {
        e.preventDefault()
        open = !open
      }
    }
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  })

  function run(action: () => void) {
    open = false
    search = ''
    // Tick delay so the dialog closes before the action fires
    // (avoids focus conflicts with settings dialog, file picker, etc.)
    requestAnimationFrame(action)
  }
</script>

<Dialog.Root
  bind:open
  onOpenChange={(v) => {
    if (!v) search = ''
  }}
>
  <Dialog.Portal>
    <Dialog.Overlay class="fixed inset-0 z-50 bg-ground/70 backdrop-blur-sm" />
    <Dialog.Content class="fixed inset-0 z-50 flex items-start justify-center pt-[20vh]" aria-label="Command center">
      <div
        class={cn(
          'w-full max-w-[520px] overflow-hidden rounded-xl border border-edge',
          'bg-raised shadow-[0_16px_48px_rgba(0,0,0,0.5)]'
        )}
      >
        <Command loop vimBindings={false}>
          <CommandInput placeholder="Type a command..." bind:value={search} />
          <CommandList>
            <CommandEmpty />

            <CommandGroup heading="File">
              <CommandItem
                value="open-project"
                keywords={['new', 'add', 'folder', 'directory']}
                onSelect={() => run(onNewProject)}
              >
                <FolderOpenIcon />
                Open Project
                <CommandShortcut>Ctrl+O</CommandShortcut>
              </CommandItem>

              <CommandItem
                value="settings"
                keywords={['preferences', 'config', 'options']}
                onSelect={() => run(onSettings)}
              >
                <SettingsIcon />
                Settings
              </CommandItem>

              {#if activeId}
                <CommandItem
                  value="close-project"
                  keywords={['remove', 'close']}
                  onSelect={() => {
                    const id = activeId
                    if (id) run(() => closeProject(id))
                  }}
                >
                  <XIcon />
                  Close Project
                </CommandItem>
              {/if}
            </CommandGroup>

            {#if recentProjects.length > 0}
              <CommandSeparator />
              <CommandGroup heading="Recent Projects">
                {#each recentProjects as project (project.id)}
                  <CommandItem
                    value="recent-{project.id}"
                    keywords={[project.name, project.path]}
                    onSelect={() => run(() => openProject(project.id))}
                  >
                    <FolderClockIcon />
                    <span class="truncate">{project.name}</span>
                    <span class="ml-auto truncate text-xs text-subtle">{project.path}</span>
                  </CommandItem>
                {/each}
              </CommandGroup>
            {/if}

            <CommandSeparator />
            <CommandGroup heading="View">
              {#if activeId}
                <CommandItem
                  value="toggle-git-sidebar"
                  keywords={['sidebar', 'panel', 'git', 'show', 'hide']}
                  onSelect={() => run(toggleGitSidebar)}
                >
                  <PanelLeftIcon />
                  {sidebarCollapsed ? 'Show' : 'Hide'} Git Sidebar
                </CommandItem>
              {/if}

              <CommandItem
                value="zoom-in"
                keywords={['zoom', 'larger', 'bigger', 'magnify']}
                onSelect={() => run(zoomIn)}
              >
                <ZoomInIcon />
                Zoom In
                <CommandShortcut>Ctrl+=</CommandShortcut>
              </CommandItem>

              <CommandItem value="zoom-out" keywords={['zoom', 'smaller', 'shrink']} onSelect={() => run(zoomOut)}>
                <ZoomOutIcon />
                Zoom Out
                <CommandShortcut>Ctrl+-</CommandShortcut>
              </CommandItem>

              <CommandItem
                value="zoom-reset"
                keywords={['zoom', 'reset', 'default', '100%']}
                onSelect={() => run(zoomReset)}
              >
                <RotateCcwIcon />
                Reset Zoom
                <CommandShortcut>Ctrl+0</CommandShortcut>
              </CommandItem>
            </CommandGroup>
          </CommandList>
        </Command>
      </div>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>
