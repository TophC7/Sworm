<script lang="ts">
  import { Input } from '$lib/components/ui/input'
  import { Button } from '$lib/components/ui/button'
  import type { PickOption } from '$lib/features/confirm/service.svelte'

  let {
    title,
    options,
    filterPlaceholder,
    onpick
  }: {
    title: string
    options: PickOption[]
    filterPlaceholder?: string
    onpick: (value: string) => void
  } = $props()

  let filter = $state('')
  let query = $derived(filter.toLowerCase())
  let matches = $derived(
    options.filter((option) => `${option.label} ${option.description ?? ''}`.toLowerCase().includes(query))
  )

  function moveFocus(event: KeyboardEvent & { currentTarget: EventTarget & HTMLDivElement }) {
    if (event.key !== 'ArrowDown' && event.key !== 'ArrowUp') return
    event.preventDefault()
    const buttons = [...event.currentTarget.querySelectorAll<HTMLButtonElement>('[data-pick-option]')]
    if (!buttons.length) return
    const index = buttons.findIndex((option) => option === document.activeElement)
    const step = event.key === 'ArrowDown' ? 1 : -1
    const next = index < 0 ? (step === 1 ? 0 : buttons.length - 1) : (index + step + buttons.length) % buttons.length
    buttons[next].focus()
  }
</script>

<div class="mt-3 flex flex-col gap-2">
  {#if filterPlaceholder}
    <!-- svelte-ignore a11y_autofocus — a searchable modal picker starts in its filter -->
    <Input
      aria-label={filterPlaceholder}
      placeholder={filterPlaceholder}
      bind:value={filter}
      autofocus
      onkeydown={(event) => {
        if (event.key === 'Enter' && matches[0]) {
          event.preventDefault()
          onpick(matches[0].value)
        }
      }}
    />
  {/if}
  <div
    class="flex max-h-80 flex-col gap-1 overflow-y-auto"
    role="listbox"
    aria-label={title}
    tabindex="-1"
    onkeydown={moveFocus}
  >
    {#each matches as option, index (option.value)}
      <!-- svelte-ignore a11y_autofocus — without a filter the first option takes focus -->
      <Button
        type="button"
        data-pick-option
        role="option"
        aria-selected={false}
        autofocus={!filterPlaceholder && index === 0}
        class="w-full flex-col items-start justify-start border border-edge bg-overlay px-3 py-2 text-left transition-none hover:border-accent/40 hover:bg-overlay"
        onclick={() => onpick(option.value)}
      >
        <span class="text-base text-fg">{option.label}</span>
        {#if option.description}<span class="font-mono text-xs text-subtle">{option.description}</span>{/if}
      </Button>
    {:else}
      <p class="px-3 py-2 text-base text-subtle">No matches.</p>
    {/each}
  </div>
</div>
