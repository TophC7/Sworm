<script lang="ts">
  import { getSessions } from '$lib/stores/sessions.svelte'
  import { addSessionTab } from '$lib/stores/workspace.svelte'
  import { setGitSidebarCollapsed } from '$lib/stores/ui.svelte'
  import type { Session } from '$lib/types/backend'
  import { providerLabel } from '$lib/utils/session'
  import { Button } from '$lib/components/ui/button'
  import { PanelLeftClose } from '@lucide/svelte'

  let {
    projectId
  }: {
    projectId: string
  } = $props()

  let sessions = $derived(getSessions())

  // Filter out terminal and fresh — only track agent CLIs
  let agentSessions = $derived(sessions.filter((s) => s.provider_id !== 'terminal' && s.provider_id !== 'fresh'))

  // Group sessions by provider
  let grouped = $derived.by(() => {
    const groups = new Map<string, Session[]>()
    for (const session of agentSessions) {
      const existing = groups.get(session.provider_id)
      if (existing) {
        existing.push(session)
      } else {
        groups.set(session.provider_id, [session])
      }
    }
    return groups
  })

  function handleSessionClick(session: Session) {
    // addSessionTab deduplicates — focuses existing tab if present
    addSessionTab(projectId, session.id, session.title, session.provider_id)
  }
</script>

<div class="flex h-full flex-col bg-ground">
  <!-- Header -->
  <div class="flex h-8 min-h-8 shrink-0 items-center justify-between border-b border-edge bg-surface px-2.5">
    <span class="text-[0.7rem] font-semibold tracking-wide text-muted uppercase">Sessions</span>
    <Button variant="ghost" size="icon-sm" onclick={() => setGitSidebarCollapsed(true)}>
      <PanelLeftClose size={12} />
    </Button>
  </div>

  <!-- Session list -->
  <div class="flex-1 overflow-y-auto">
    {#if agentSessions.length === 0}
      <div class="px-2.5 py-3 text-[0.75rem] text-subtle">No agent sessions yet.</div>
    {:else}
      {#each [...grouped.entries()] as [providerId, providerSessions]}
        <div class="border-b border-edge last:border-b-0">
          <div class="px-2.5 py-1.5 text-[0.65rem] font-semibold tracking-wider text-muted uppercase">
            {providerLabel(providerId)}
          </div>
          {#each providerSessions as session}
            <button
              class="group flex w-full cursor-pointer items-center gap-2 px-2.5 py-1.5 text-left text-[0.75rem] text-subtle transition-colors hover:bg-surface hover:text-bright"
              onclick={() => handleSessionClick(session)}
            >
              <!-- Status dot -->
              <span
                class="h-1.5 w-1.5 shrink-0 rounded-full"
                class:bg-success={session.status === 'running'}
                class:bg-warning={session.status === 'starting'}
                class:bg-muted={session.status === 'idle' ||
                  session.status === 'stopped' ||
                  session.status === 'exited'}
                class:bg-danger={session.status === 'failed'}
              ></span>
              <span class="truncate font-mono text-[0.7rem]">{session.id.slice(0, 8)}</span>
            </button>
          {/each}
        </div>
      {/each}
    {/if}
  </div>
</div>
