<!--
  @component
  IssueDetailForm. Body of the IssueSurface, mounted under
  {#key detail.issue.id} so $state initializers (and useDetailDraft's
  baseline) re-seed cleanly when the active tab id changes. No state
  mirroring via $effect.
-->

<script lang="ts">
  import { untrack } from 'svelte'
  import { Button } from '$lib/components/ui/button'
  import { Input, Select, Textarea } from '$lib/components/ui/input'
  import {
    BreadcrumbItem,
    BreadcrumbList,
    BreadcrumbPage,
    BreadcrumbRoot,
    BreadcrumbSeparator
  } from '$lib/components/ui/breadcrumb'
  import { Layers, MessageSquare } from '$lib/icons/lucideExports'
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
  import SectionHeading from '$lib/features/issues/SectionHeading.svelte'
  import { formatFullDate, timeAgo } from '$lib/utils/date'
  import type { Issue, IssueDetail, IssueStatus } from '$lib/types/backend'
  import type { IssueTab } from '$lib/features/workbench/model'

  let {
    detail,
    projectId,
    tab
  }: {
    detail: IssueDetail
    projectId: string
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
      const next = await updateIssue(projectId, tab.issueId, {
        title: drafts.title.trim(),
        description: drafts.description.trim() || null,
        status: drafts.status,
        priority: Number(drafts.priority),
        tags: parseTags(drafts.tags)
      })
      if (next.title !== tab.title) {
        updateIssueTabTitle(projectId, tab.issueId, next.title)
      }
    }
  })

  let commentDraft = $state('')

  let allIssues = $derived(getIssues(projectId))
  let allEpics = $derived(getIssueEpics(projectId))
  let parentIssue = $derived(
    detail.issue.parentIssueId ? (allIssues.find((i) => i.id === detail.issue.parentIssueId) ?? null) : null
  )
  let epicRef = $derived(detail.issue.epicId ? (allEpics.find((e) => e.id === detail.issue.epicId) ?? null) : null)

  function parseTags(raw: string): string[] {
    return raw
      .split(',')
      .map((t) => t.trim())
      .filter(Boolean)
  }

  async function postComment() {
    if (!commentDraft.trim()) return
    await addIssueComment(projectId, tab.issueId, commentDraft)
    commentDraft = ''
  }

  function openSibling(id: string, title: string) {
    void openIssueTab(projectId, id, title)
  }

  function handleSelectIssue(issue: Issue) {
    openSibling(issue.id, issue.title)
  }
</script>

{#snippet depRow(label: string, ids: string[])}
  <span class="text-2xs tracking-wider text-muted uppercase">{label}</span>
  <span class="font-mono text-2xs">
    {#if ids.length === 0}
      <span class="font-sans text-subtle italic">none</span>
    {:else}
      {#each ids as id, i (id)}
        <button type="button" class="text-accent hover:underline" onclick={() => openSibling(id, id)}>
          {id}
        </button>
        {#if i < ids.length - 1}
          <span class="text-subtle">,</span>
        {/if}
      {/each}
    {/if}
  </span>
{/snippet}

<div class="min-h-0 flex-1 overflow-y-auto">
  <div class="mx-auto flex w-full max-w-3xl flex-col gap-5 px-5 py-5">
    <BreadcrumbRoot>
      <BreadcrumbList class="text-2xs">
        {#if epicRef}
          <BreadcrumbItem class="font-mono text-warning" title={epicRef.title}>
            <Layers size={10} class="shrink-0" />
            <span>{epicRef.id}</span>
            <span class="max-w-[16ch] truncate font-sans text-subtle">{epicRef.title}</span>
          </BreadcrumbItem>
          <BreadcrumbSeparator />
        {/if}
        {#if parentIssue}
          <BreadcrumbItem class="font-mono">
            <button
              type="button"
              class="flex items-center gap-1 text-accent hover:underline focus-visible:shadow-focus-ring focus-visible:outline-none"
              onclick={() => openSibling(parentIssue!.id, parentIssue!.title)}
              title={parentIssue.title}
            >
              <span>{parentIssue.id}</span>
              <span class="max-w-[16ch] truncate font-sans text-subtle">{parentIssue.title}</span>
            </button>
          </BreadcrumbItem>
          <BreadcrumbSeparator />
        {/if}
        <BreadcrumbItem>
          <BreadcrumbPage class="font-mono">{detail.issue.id}</BreadcrumbPage>
        </BreadcrumbItem>
      </BreadcrumbList>
    </BreadcrumbRoot>

    <Input bind:value={form.drafts.title} class="text-base" />

    <SectionHeading label="Properties" />
    <dl class="grid grid-cols-[110px_minmax(0,1fr)] items-center gap-x-3 gap-y-2 text-sm">
      <dt class="text-2xs tracking-wider text-muted uppercase">Status</dt>
      <dd class="flex items-center gap-2">
        <Select bind:value={form.drafts.status} class="max-w-[200px]">
          {#each ALL_STATUSES as status}
            <option value={status}>{statusLabel(status)}</option>
          {/each}
        </Select>
        <span class="text-2xs {statusGlyphTone(detail.issue.status)}">
          now: {statusLabel(detail.issue.status)}
        </span>
      </dd>

      <dt class="text-2xs tracking-wider text-muted uppercase">Priority</dt>
      <dd class="flex items-center gap-2">
        <Select bind:value={form.drafts.priority} class="max-w-[100px]">
          {#each ALL_PRIORITIES as p}
            <option value={String(p)}>P{p}</option>
          {/each}
        </Select>
        <span class="font-mono text-2xs {priorityToneClass(detail.issue.priority)}">
          now: P{detail.issue.priority}
        </span>
      </dd>

      <dt class="text-2xs tracking-wider text-muted uppercase">Tags</dt>
      <dd>
        <Input bind:value={form.drafts.tags} placeholder="comma, separated, tags" />
      </dd>

      <dt class="text-2xs tracking-wider text-muted uppercase">Epic</dt>
      <dd class="font-mono text-2xs">
        {#if epicRef}
          <span class="text-warning">◆ {epicRef.id}</span>
          <span class="ml-1 font-sans text-subtle">{epicRef.title}</span>
        {:else}
          <span class="font-sans text-subtle italic">none</span>
        {/if}
      </dd>

      <dt class="text-2xs tracking-wider text-muted uppercase">Parent</dt>
      <dd class="font-mono text-2xs">
        {#if parentIssue}
          <button
            type="button"
            class="text-accent hover:underline focus-visible:shadow-focus-ring focus-visible:outline-none"
            onclick={() => openSibling(parentIssue!.id, parentIssue!.title)}
          >
            ↑ {parentIssue.id}
          </button>
          <span class="ml-1 font-sans text-subtle">{parentIssue.title}</span>
        {:else}
          <span class="font-sans text-subtle italic">none</span>
        {/if}
      </dd>

      <dt class="text-2xs tracking-wider text-muted uppercase">Created</dt>
      <dd class="text-2xs text-muted" title={detail.issue.createdAt}>
        {formatFullDate(detail.issue.createdAt)}
      </dd>

      <dt class="text-2xs tracking-wider text-muted uppercase">Updated</dt>
      <dd class="text-2xs text-muted" title={formatFullDate(detail.issue.updatedAt)}>
        {timeAgo(detail.issue.updatedAt)}
      </dd>
    </dl>

    <SectionHeading label="Description" />
    <Textarea
      rows={6}
      bind:value={form.drafts.description}
      placeholder="Describe the work, the why, links, anything reviewers will want."
    />

    <SectionHeading label="Dependencies" />
    <div class="grid grid-cols-[110px_minmax(0,1fr)] gap-x-3 gap-y-1.5 text-sm">
      {@render depRow(
        'Depends on',
        detail.dependsOn.map((d) => d.dependsOnIssueId)
      )}
      {@render depRow(
        'Blocks',
        detail.blockedBy.map((d) => d.issueId)
      )}
    </div>

    <SectionHeading label={`Sub-issues (${detail.subIssues.length})`} />
    {#if detail.subIssues.length === 0}
      <p class="text-xs text-subtle italic">No sub-issues.</p>
    {:else}
      <ul class="flex flex-col">
        {#each detail.subIssues as sub (sub.id)}
          <IssueListRow issue={sub} onSelect={handleSelectIssue} />
        {/each}
      </ul>
    {/if}

    <SectionHeading label={`Comments (${detail.comments.length})`} />
    {#if detail.comments.length === 0}
      <p class="text-xs text-subtle italic">No notes yet.</p>
    {:else}
      <div class="flex flex-col gap-2">
        {#each detail.comments as comment (comment.id)}
          <article class="rounded-md border border-edge bg-surface px-3 py-2">
            <header class="mb-1 flex items-center justify-between gap-2 text-2xs text-muted">
              <span class="flex items-center gap-1.5">
                <MessageSquare size={10} class="text-subtle" />
                <span class="font-mono">{comment.author}</span>
              </span>
              <time title={comment.createdAt}>{timeAgo(comment.createdAt)}</time>
            </header>
            <p class="text-sm whitespace-pre-wrap text-fg">{comment.body}</p>
          </article>
        {/each}
      </div>
    {/if}
    <div class="flex flex-col gap-2">
      <Textarea rows={3} bind:value={commentDraft} placeholder="Add a note…" />
      <div class="flex justify-end">
        <Button size="sm" onclick={postComment} disabled={!commentDraft.trim()}>Post note</Button>
      </div>
    </div>

    <div class="mt-2 flex items-center justify-end gap-1.5 border-t border-edge pt-3">
      <Button size="sm" onclick={() => claimIssue(projectId, tab.issueId)}>Claim</Button>
      <Button size="sm" variant="accent" onclick={form.save} disabled={form.saving || !form.dirty}>
        {form.saving ? 'Saving…' : 'Save'}
      </Button>
    </div>
  </div>
</div>
