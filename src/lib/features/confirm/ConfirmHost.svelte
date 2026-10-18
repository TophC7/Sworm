<script lang="ts">
  import * as Dialog from '$lib/components/ui/dialog'
  import { Button } from '$lib/components/ui/button'
  import { Input } from '$lib/components/ui/input'
  import type { EventHandler } from 'svelte/elements'
  import { getPendingRequest, resolvePendingAnswer, resolvePendingConfirm } from '$lib/features/confirm/service.svelte'
  import PickList from '$lib/features/confirm/PickList.svelte'

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
          <PickList
            title={pending.title}
            options={pending.options}
            filterPlaceholder={pending.filterPlaceholder}
            onpick={resolvePendingAnswer.bind(null, pending.id)}
          />
          <Dialog.Footer>
            <Button variant="outline" onclick={resolvePendingAnswer.bind(null, pending.id, null)}>Cancel</Button>
          </Dialog.Footer>
        {/if}
      </Dialog.Content>
    </Dialog.Root>
  {/key}
{/if}
