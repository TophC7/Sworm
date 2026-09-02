<!--
  @component
  IssueDetailForm. Two-column issue detail surface: main column carries
  title, description (markdown render with click-to-edit), sub-issues,
  and a chronological activity log (audit events + comments). The right
  sidebar carries metadata (status, priority, tags, epic, parent),
  dependencies, dates, and the Claim/Save actions. Mounted under
  {#key detail.issue.id} so $state initializers re-seed cleanly when
  the active tab id changes; no $effect mirroring.
-->

<script lang="ts">
  import { untrack } from 'svelte'
  import { Button } from '$lib/components/ui/button'
  import { Input, Select, Textarea } from '$lib/components/ui/input'
  import { DetailPanel, DetailPanelRow } from '$lib/components/ui/detail-panel'
  import { CircleDot, GitBranchIcon, Hash, Layers, MessageSquare, SparklesIcon } from '$lib/icons/lucideExports'
  import { addIssueComment, claimIssue, getIssueEpics, getIssues, updateIssue } from '$lib/features/issues/state.svelte'
  import { updateIssueTabTitle } from '$lib/features/workbench/state.svelte'
  import { openIssueTab } from '$lib/features/workbench/surfaces/issue/service.svelte'
  import {
    ALL_PRIORITIES,
    ALL_STATUSES,
    priorityToneClass,
    statusGlyphTone,
    statusLabel
  } from '$lib/features/issues/visual'
  import { useDetailDraft } from '$lib/features/issues/useDetailDraft.svelte'
  import IssueListRow from '$lib/features/issues/IssueListRow.svelte'
  import MarkdownEditField from '$lib/features/issues/MarkdownEditField.svelte'
  import SectionHeading from '$lib/features/issues/SectionHeading.svelte'
  import { formatFullDate, timeAgo } from '$lib/utils/date'
  import type { Issue, IssueComment, IssueDetail, IssueEvent, IssueStatus } from '$lib/types/backend'
  import type { IssueTab } from '$lib/features/workbench/model'

  let {
    detail,
    folderPath,
    tab
  }: {
    detail: IssueDetail
    folderPath: string
    tab: IssueTab
  } = $props()

  type Draft = {
    title: string
    description: string
    status: IssueStatus
    priority: string
    // Raw comma-separated buffer; parsed into string[] at save time.
    tags: string
  }

  // Snapshot once at mount; parent re-keys on id change so this stays
  // synchronized with the active row without subscribing to detail.
  const baseline = untrack<Draft>(() => ({
    title: detail.issue.title,
    description: detail.issue.description ?? '',
    status: detail.issue.status,
    priority: String(detail.issue.priority),
    tags: detail.issue.tags.join(', ')
  }))

  const form = useDetailDraft<Draft>({
    initial: baseline,
    isDirty: (d, base) =>
      d.title.trim() !== base.title ||
      (d.description.trim() || null) !== (base.description.trim() || null) ||
      d.status !== base.status ||
      Number(d.priority) !== Number(base.priority) ||
      parseTags(d.tags).join(',') !== detail.issue.tags.join(','),
    save: async (drafts) => {
      const next = await updateIssue(folderPath, tab.issueId, {
        title: drafts.title.trim(),
        description: drafts.description.trim() || null,
        status: drafts.status,
        priority: Number(drafts.priority),
        tags: parseTags(drafts.tags)
      })
      if (next.title !== tab.title) {
        updateIssueTabTitle(folderPath, tab.issueId, next.title)
      }
    }
  })

  let commentDraft = $state('')

  let allIssues = $derived(getIssues(folderPath))
  let allEpics = $derived(getIssueEpics(folderPath))
  let parentIssue = $derived(
    detail.issue.parentIssueId ? (allIssues.find((i) => i.id === detail.issue.parentIssueId) ?? null) : null
  )
  let epicRef = $derived(detail.issue.epicId ? (allEpics.find((e) => e.id === detail.issue.epicId) ?? null) : null)

  // Merge audit events with comments into a single chronological feed.
  // Backend scopes events by entity_id so comment/dependency events
  // don't leak in here.
  type ActivityItem =
    { kind: 'comment'; createdAt: string; data: IssueComment } | { kind: 'event'; createdAt: string; data: IssueEvent }

  let activity = $derived.by<ActivityItem[]>(() => {
    const items: ActivityItem[] = []
    for (const c of detail.comments) {
      items.push({ kind: 'comment', createdAt: c.createdAt, data: c })
    }
    for (const e of detail.events) {
      items.push({ kind: 'event', createdAt: e.createdAt, data: e })
    }
    items.sort((a, b) => a.createdAt.localeCompare(b.createdAt))
    return items
  })

  function parseTags(raw: string): string[] {
    return raw
      .split(',')
      .map((t) => t.trim())
      .filter(Boolean)
  }

  function eventLabel(e: IssueEvent): string {
    if (e.action === 'create') return 'created the issue'
    if (e.action === 'update') return 'edited the issue'
    if (e.action === 'delete') return 'archived the issue'
    return `${e.action} ${e.entityType}`
  }

  async function postComment() {
    if (!commentDraft.trim()) return
    await addIssueComment(folderPath, tab.issueId, commentDraft)
    commentDraft = ''
  }
</script>

{#snippet depList(ids: string[])}
  {#if ids.length === 0}
    <span class="text-2xs text-subtle italic">none</span>
  {:else}
    <div class="flex flex-wrap gap-x-1 gap-y-0.5 font-mono text-2xs">
      {#each ids as id, i (id)}
        <button
          type="button"
          class="text-accent hover:underline focus-visible:shadow-focus-ring focus-visible:outline-none"
          onclick={() => openIssueTab(folderPath, id, id)}
        >
          {id}
        </button>
        {#if i < ids.length - 1}
          <span class="text-subtle">,</span>
        {/if}
      {/each}
    </div>
  {/if}
{/snippet}

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
        editPlaceholder="Describe the work, the why, links, anything reviewers will want."
      />

      <SectionHeading label={`Sub-issues (${detail.subIssues.length})`} />
      {#if detail.subIssues.length === 0}
        <p class="text-xs text-subtle italic">No sub-issues.</p>
      {:else}
        <ul class="flex flex-col">
          {#each detail.subIssues as sub (sub.id)}
            <IssueListRow issue={sub} onSelect={(issue: Issue) => openIssueTab(folderPath, issue.id, issue.title)} />
          {/each}
        </ul>
      {/if}

      <SectionHeading label="Activity" />
      {#if activity.length === 0}
        <p class="text-xs text-subtle italic">No activity yet.</p>
      {:else}
        <ul class="flex flex-col">
          {#each activity as item, i (item.kind + ':' + item.data.id)}
            <!-- min-h-14 keeps short events tall enough that the rail
                 has room and adjacent circles never touch -->
            <li class="relative flex min-h-14 gap-3 pb-3 last:min-h-0 last:pb-0">
              {#if i < activity.length - 1}
                <span
                  class="pointer-events-none absolute top-3.5 bottom-0 left-3.5 w-px -translate-x-1/2 bg-edge"
                  aria-hidden="true"
                ></span>
              {/if}
              <span
                class="relative z-10 flex h-7 w-7 shrink-0 items-center justify-center rounded-full border border-edge bg-ground"
              >
                {#if item.kind === 'comment'}
                  <MessageSquare size={13} class="text-accent" />
                {:else if item.data.action === 'create'}
                  <SparklesIcon size={13} class="text-success" />
                {:else}
                  <CircleDot size={12} class="text-muted" />
                {/if}
              </span>
              <div class="min-w-0 flex-1">
                {#if item.kind === 'comment'}
                  <article class="rounded-md border border-edge bg-surface px-3 py-2">
                    <header class="mb-1 flex items-center justify-between gap-2 text-2xs text-muted">
                      <span class="font-mono">{item.data.author}</span>
                      <time title={formatFullDate(item.data.createdAt)}>
                        {timeAgo(item.data.createdAt)}
                      </time>
                    </header>
                    <p class="text-sm whitespace-pre-wrap text-fg">{item.data.body}</p>
                  </article>
                {:else}
                  <p class="flex flex-wrap items-baseline gap-x-2 gap-y-0.5 pt-1.5 text-2xs text-muted">
                    <span class="font-mono text-subtle">{item.data.actor}</span>
                    <span>{eventLabel(item.data)}</span>
                    <time class="text-subtle" title={formatFullDate(item.data.createdAt)}>
                      {timeAgo(item.data.createdAt)}
                    </time>
                  </p>
                {/if}
              </div>
            </li>
          {/each}
        </ul>
      {/if}

      <div class="flex flex-col gap-2">
        <Textarea rows={3} bind:value={commentDraft} placeholder="Add a note…" />
        <div class="flex justify-end">
          <Button size="sm" onclick={postComment} disabled={!commentDraft.trim()}>Post note</Button>
        </div>
      </div>
    </main>

    <aside class="w-full @3xl:w-72 @3xl:shrink-0">
      <DetailPanel>
        <DetailPanelRow label="Status">
          <Select bind:value={form.drafts.status} class="w-full">
            {#each ALL_STATUSES as status (status)}
              <option value={status}>{statusLabel(status)}</option>
            {/each}
          </Select>
          <span class="text-2xs {statusGlyphTone(detail.issue.status)}">
            now: {statusLabel(detail.issue.status)}
          </span>
        </DetailPanelRow>

        <DetailPanelRow label="Priority">
          <Select bind:value={form.drafts.priority} class="w-full">
            {#each ALL_PRIORITIES as p (p)}
              <option value={String(p)}>P{p}</option>
            {/each}
          </Select>
          <span class="font-mono text-2xs {priorityToneClass(detail.issue.priority)}">
            now: P{detail.issue.priority}
          </span>
        </DetailPanelRow>

        <DetailPanelRow label="Tags">
          {#snippet icon()}<Hash size={10} />{/snippet}
          <Input bind:value={form.drafts.tags} placeholder="comma, separated" />
        </DetailPanelRow>

        <DetailPanelRow label="Epic">
          {#snippet icon()}<Layers size={10} />{/snippet}
          {#if epicRef}
            <span class="font-mono text-2xs text-warning">◆ {epicRef.id}</span>
            <span class="truncate text-2xs text-fg" title={epicRef.title}>{epicRef.title}</span>
          {:else}
            <span class="text-2xs text-subtle italic">none</span>
          {/if}
        </DetailPanelRow>

        <DetailPanelRow label="Parent">
          {#snippet icon()}<GitBranchIcon size={10} />{/snippet}
          {#if parentIssue}
            <button
              type="button"
              class="text-left font-mono text-2xs text-accent hover:underline focus-visible:shadow-focus-ring focus-visible:outline-none"
              onclick={() => openIssueTab(folderPath, parentIssue!.id, parentIssue!.title)}
            >
              ↑ {parentIssue.id}
            </button>
            <span class="truncate text-2xs text-fg" title={parentIssue.title}>
              {parentIssue.title}
            </span>
          {:else}
            <span class="text-2xs text-subtle italic">none</span>
          {/if}
        </DetailPanelRow>

        <DetailPanelRow label="Depends on">
          {@render depList(detail.dependsOn.map((d) => d.dependsOnIssueId))}
        </DetailPanelRow>

        <DetailPanelRow label="Blocks">
          {@render depList(detail.blockedBy.map((d) => d.issueId))}
        </DetailPanelRow>

        <DetailPanelRow label="Created" dense>
          <span class="text-2xs text-muted" title={detail.issue.createdAt}>
            {formatFullDate(detail.issue.createdAt)}
          </span>
        </DetailPanelRow>

        <DetailPanelRow label="Updated" dense>
          <span class="text-2xs text-muted" title={formatFullDate(detail.issue.updatedAt)}>
            {timeAgo(detail.issue.updatedAt)}
          </span>
        </DetailPanelRow>
      </DetailPanel>

      <div class="mt-3 flex flex-col gap-1.5">
        <Button size="sm" variant="accent" onclick={form.save} disabled={form.saving || !form.dirty}>
          {form.saving ? 'Saving…' : 'Save'}
        </Button>
        <Button size="sm" onclick={() => claimIssue(folderPath, tab.issueId)}>Claim</Button>
      </div>
    </aside>
  </div>
</div>
