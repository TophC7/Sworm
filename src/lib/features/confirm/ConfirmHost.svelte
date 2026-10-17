<script lang="ts">
  import * as Dialog from '$lib/components/ui/dialog'
  import { Button } from '$lib/components/ui/button'
  import { Input } from '$lib/components/ui/input'
  import type { EventHandler } from 'svelte/elements'
  import { getPendingRequest, resolvePendingAnswer, resolvePendingConfirm } from '$lib/features/confirm/service.svelte'

  const pending = $derived(getPendingRequest())

  function dismissRequest(id: number, confirm: boolean) {
    return (open: boolean) => {
      if (!open) {
        if (confirm) void resolvePendingConfirm(id, false)
        else resolvePendingAnswer(id, null)
      }
    }
  }

  function submitAnswer(id: number): EventHandler<SubmitEvent, HTMLFormElement> {
    return (event) => {
      event.preventDefault()
      resolvePendingAnswer(id, String(new FormData(event.currentTarget).get('answer') ?? ''))
    }
  }

  function moveFocus(event: KeyboardEvent & { currentTarget: EventTarget & HTMLDivElement }) {
    if (event.key !== 'ArrowDown' && event.key !== 'ArrowUp') return
    event.preventDefault()
    const options = [...event.currentTarget.querySelectorAll<HTMLButtonElement>('[data-pick-option]')]
    if (!options.length) return
    const index = options.findIndex((option) => option === document.activeElement)
    const step = event.key === 'ArrowDown' ? 1 : -1
    const next = index < 0 ? (step === 1 ? 0 : options.length - 1) : (index + step + options.length) % options.length
    options[next].focus()
  }
</script>

{#if pending}
  <!-- Each request gets fresh open state; callbacks capture its id before a queue advance. -->
  {#key pending.id}
    <Dialog.Root open={true} onOpenChange={dismissRequest(pending.id, pending.kind === 'confirm')}>
      <Dialog.Content
        escapeKeydownBehavior={pending.busy ? 'ignore' : 'close'}
        interactOutsideBehavior={pending.busy ? 'ignore' : 'close'}
        aria-busy={pending.busy}
      >
        <Dialog.Title>{pending.title}</Dialog.Title>
        {#if pending.message}<Dialog.Description>{pending.message}</Dialog.Description>{/if}
        {#if pending.kind === 'confirm'}
          <Dialog.Footer>
            <Button
              variant="outline"
              disabled={pending.busy}
              onclick={resolvePendingConfirm.bind(null, pending.id, false)}
            >
              {pending.cancelLabel ?? 'Cancel'}
            </Button>
            <Button
              variant="accent"
              disabled={pending.busy}
              onclick={resolvePendingConfirm.bind(null, pending.id, true)}
            >
              {pending.confirmLabel ?? 'Confirm'}
            </Button>
          </Dialog.Footer>
        {:else if pending.kind === 'prompt'}
          <form onsubmit={submitAnswer(pending.id)}>
            <!-- svelte-ignore a11y_autofocus — focusing the sole input on mount is expected UX for a modal prompt -->
            <Input
              name="answer"
              class="mt-2"
              aria-label={pending.title}
              value={pending.value ?? ''}
              placeholder={pending.placeholder ?? ''}
              autofocus
            />
            <Dialog.Footer>
              <Button type="button" variant="outline" onclick={resolvePendingAnswer.bind(null, pending.id, null)}>
                Cancel
              </Button>
              <Button type="submit" variant="accent">{pending.confirmLabel ?? 'OK'}</Button>
            </Dialog.Footer>
          </form>
        {:else}
          <div
            class="mt-3 flex flex-col gap-1"
            role="listbox"
            aria-label={pending.title}
            tabindex="-1"
            onkeydown={moveFocus}
          >
            {#each pending.options as option, index}
              <!-- svelte-ignore a11y_autofocus — focusing the first option on mount is expected UX for a modal picker -->
              <button
                type="button"
                data-pick-option
                role="option"
                aria-selected={false}
                autofocus={index === 0}
                class="flex w-full flex-col items-start rounded-lg border border-edge bg-overlay px-3 py-2 text-left hover:border-accent/40 hover:text-bright focus-visible:shadow-focus-ring focus-visible:outline-none"
                onclick={resolvePendingAnswer.bind(null, pending.id, option.value)}
              >
                <span class="text-base text-fg">{option.label}</span>
                {#if option.description}<span class="font-mono text-xs text-subtle">{option.description}</span>{/if}
              </button>
            {/each}
          </div>
          <Dialog.Footer>
            <Button variant="outline" onclick={resolvePendingAnswer.bind(null, pending.id, null)}>Cancel</Button>
          </Dialog.Footer>
        {/if}
      </Dialog.Content>
    </Dialog.Root>
  {/key}
{/if}
