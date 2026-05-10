<!--
  @component
  EpicDetailForm. Two-column epic detail surface mirroring the issue
  surface: main column carries title, description (markdown render with
  click-to-edit), and the child-issue list; the right sidebar carries
  status, priority, dates, and the Save action. Mounted under
  {#key detail.id} so $state initializers re-seed cleanly when the
  active tab id changes.
-->

<script lang="ts">
  import { untrack } from 'svelte'
  import { Button } from '$lib/components/ui/button'
  import { Input, Select } from '$lib/components/ui/input'
  import { DetailPanel, DetailPanelRow } from '$lib/components/ui/detail-panel'
  import { getIssues, updateEpic } from '$lib/features/issues/state.svelte'
  import { updateEpicTabTitle } from '$lib/features/workbench/state.svelte'
  import { openIssueTab } from '$lib/features/workbench/surfaces/issue/service.svelte'
  import { ALL_PRIORITIES, priorityToneClass, statusGlyphTone, statusLabel } from '$lib/features/issues/visual'
  import { useDetailDraft } from '$lib/features/issues/useDetailDraft.svelte'
  import IssueListRow from '$lib/features/issues/IssueListRow.svelte'
  import MarkdownEditField from '$lib/features/issues/MarkdownEditField.svelte'
  import SectionHeading from '$lib/features/issues/SectionHeading.svelte'
  import { formatFullDate, timeAgo } from '$lib/utils/date'
  import type { Issue, IssueEpic, IssueEpicStatus, IssueStatus } from '$lib/types/backend'
  import type { EpicTab } from '$lib/features/workbench/model'

  let {
    detail,
    projectId,
    tab
  }: {
    detail: IssueEpic
    projectId: string
    tab: EpicTab
  } = $props()

  const ALL_EPIC_STATUSES: IssueEpicStatus[] = ['todo', 'in_progress', 'completed', 'archived']

  type Draft = {
    title: string
    description: string
    status: IssueEpicStatus
    priority: string
  }

  // Snapshot once at mount; parent re-keys on id change.
  const baseline = untrack<Draft>(() => ({
    title: detail.title,
    description: detail.description ?? '',
    status: detail.status,
    priority: String(detail.priority)
  }))

  const form = useDetailDraft<Draft>({
    initial: baseline,
    isDirty: (d, base) =>
      d.title.trim() !== base.title ||
      (d.description.trim() || null) !== (base.description.trim() || null) ||
      d.status !== base.status ||
      Number(d.priority) !== Number(base.priority),
    save: async (drafts) => {
      const next = await updateEpic(projectId, tab.epicId, {
        title: drafts.title.trim(),
        description: drafts.description.trim() || null,
        status: drafts.status,
        priority: Number(drafts.priority)
      })
      if (next.title !== tab.title) {
        updateEpicTabTitle(projectId, tab.epicId, next.title)
      }
    }
  })

  let allIssues = $derived(getIssues(projectId))
  let epicIssues = $derived(allIssues.filter((i) => i.epicId === tab.epicId))

  function openIssue(issue: Issue) {
    void openIssueTab(projectId, issue.id, issue.title)
  }
</script>

<div class="@container min-h-0 flex-1 overflow-y-auto">
  <div class="flex flex-col gap-6 px-5 py-5 @3xl:flex-row">
    <main class="flex min-w-0 flex-1 flex-col gap-5">
      <Input
        bind:value={form.drafts.title}
        spellcheck="false"
        class="border-transparent bg-transparent px-2 py-1 text-2xl font-semibold text-bright hover:bg-surface focus:bg-surface"
      />

      <MarkdownEditField
        bind:value={form.drafts.description}
        editPlaceholder="Describe the goal, scope, and links for this epic."
      />

      <SectionHeading label={`Issues (${epicIssues.length})`} />
      {#if epicIssues.length === 0}
        <p class="text-xs text-subtle italic">No issues in this epic yet.</p>
      {:else}
        <ul class="flex flex-col">
          {#each epicIssues as issue (issue.id)}
            <IssueListRow {issue} onSelect={openIssue} />
          {/each}
        </ul>
      {/if}
    </main>

    <aside class="w-full @3xl:w-72 @3xl:shrink-0">
      <DetailPanel>
        <DetailPanelRow label="Status">
          <Select bind:value={form.drafts.status} class="w-full">
            {#each ALL_EPIC_STATUSES as status (status)}
              <option value={status}>{statusLabel(status)}</option>
            {/each}
          </Select>
          <span class="text-2xs {statusGlyphTone(detail.status as IssueStatus)}">
            now: {statusLabel(detail.status)}
          </span>
        </DetailPanelRow>

        <DetailPanelRow label="Priority">
          <Select bind:value={form.drafts.priority} class="w-full">
            {#each ALL_PRIORITIES as p (p)}
              <option value={String(p)}>P{p}</option>
            {/each}
          </Select>
          <span class="font-mono text-2xs {priorityToneClass(detail.priority)}">
            now: P{detail.priority}
          </span>
        </DetailPanelRow>

        <DetailPanelRow label="Created" dense>
          <span class="text-2xs text-muted" title={detail.createdAt}>
            {formatFullDate(detail.createdAt)}
          </span>
        </DetailPanelRow>

        <DetailPanelRow label="Updated" dense>
          <span class="text-2xs text-muted" title={formatFullDate(detail.updatedAt)}>
            {timeAgo(detail.updatedAt)}
          </span>
        </DetailPanelRow>
      </DetailPanel>

      <div class="mt-3 flex flex-col gap-1.5">
        <Button size="sm" variant="accent" onclick={form.save} disabled={form.saving || !form.dirty}>
          {form.saving ? 'Saving…' : 'Save'}
        </Button>
      </div>
    </aside>
  </div>
</div>
