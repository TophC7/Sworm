<!--
  @component
  IssueQuickCapture. Slide-down composer for new issue/epic.

  Visible only while `open === true`. Owns its own draft text but
  delegates persistence to the parent via `onSubmit`. Esc cancels,
  Enter commits.
-->

<script lang="ts">
  import { Input, Select } from '$lib/components/ui/input'
  import { Button } from '$lib/components/ui/button'
  import type { IssueEpic } from '$lib/types/backend'

  type Mode = 'issue' | 'epic'

  let {
    open,
    mode = 'issue',
    epics = [],
    epicId = $bindable(''),
    onSubmit,
    onCancel
  }: {
    open: boolean
    mode?: Mode
    epics?: IssueEpic[]
    epicId?: string
    onSubmit: (title: string) => void | Promise<void>
    onCancel: () => void
  } = $props()

  let value = $state('')
  let inputEl = $state<HTMLInputElement | null>(null)

  // Edge-trigger: clear draft + focus on the false→true transition.
  // Tracking `prevOpen` keeps subsequent re-renders from yanking focus
  // mid-typing.
  let prevOpen = false
  $effect(() => {
    if (open && !prevOpen) {
      value = ''
      inputEl?.focus()
    }
    prevOpen = open
  })

  async function submit() {
    const trimmed = value.trim()
    if (!trimmed) return
    if (mode === 'issue' && !epicId) return
    await onSubmit(trimmed)
    value = ''
  }

  function onkeydown(event: KeyboardEvent) {
    if (event.key === 'Enter') {
      event.preventDefault()
      void submit()
    } else if (event.key === 'Escape') {
      event.preventDefault()
      onCancel()
    }
  }
</script>

{#if open}
  <div class="flex flex-col gap-1 border-b border-edge bg-surface px-2 py-2">
    <div class="flex items-center gap-1.5">
      <span class="font-mono text-2xs tracking-wider uppercase {mode === 'epic' ? 'text-warning' : 'text-accent'}">
        {mode === 'epic' ? 'NEW EPIC' : 'NEW ISSUE'}
      </span>
      <span class="ml-auto text-2xs text-subtle">↵ create · esc cancel</span>
    </div>
    {#if mode === 'issue'}
      <Select bind:value={epicId} class="h-7 text-sm" aria-label="Issue epic">
        <option value="">Choose epic…</option>
        {#each epics as epic (epic.id)}
          <option value={epic.id}>{epic.id} · {epic.title}</option>
        {/each}
      </Select>
      {#if epics.length === 0}
        <p class="text-2xs text-warning">Create an epic before adding issues.</p>
      {/if}
    {/if}
    <Input
      bind:ref={inputEl}
      bind:value
      placeholder={mode === 'epic' ? 'Epic title…' : 'What needs doing?'}
      disabled={mode === 'issue' && !epicId}
      {onkeydown}
    />
    <div class="flex justify-end gap-1.5">
      <Button size="xs" variant="ghost" onclick={onCancel}>Cancel</Button>
      <Button size="xs" variant="accent" onclick={submit} disabled={!value.trim() || (mode === 'issue' && !epicId)}>
        Create
      </Button>
    </div>
  </div>
{/if}
