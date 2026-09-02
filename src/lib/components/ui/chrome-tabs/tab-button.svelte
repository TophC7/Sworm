<!-- Global title-bar tab button with active styling, beam, and close affordance. -->
<script lang="ts">
  import TabBeam from '$lib/components/ui/tab-beam.svelte'
  import { cn } from '$lib/utils/cn'
  import { X } from '$lib/icons/lucideExports'
  import type { Snippet } from 'svelte'
  import type { HTMLButtonAttributes } from 'svelte/elements'

  type CloseEvent = MouseEvent | KeyboardEvent

  let {
    active = false,
    leading,
    onClose,
    class: className,
    children,
    ...rest
  }: HTMLButtonAttributes & {
    active?: boolean
    leading?: Snippet
    onClose?: (event: CloseEvent) => void | Promise<void>
    class?: string
    children?: Snippet
  } = $props()

  function handleClose(event: CloseEvent) {
    event.stopPropagation()
    if (onClose) void onClose(event)
  }
</script>

<button
  class={cn(
    'group relative flex h-full shrink-0 cursor-pointer items-center gap-1.5 border-none px-3 text-sm focus-visible:shadow-focus-ring focus-visible:outline-none',
    active ? 'bg-raised text-bright' : 'bg-transparent text-muted hover:bg-raised/50 hover:text-bright',
    className
  )}
  role="tab"
  aria-selected={active}
  {...rest}
>
  {#if active}<TabBeam />{/if}
  {#if leading}{@render leading()}{/if}
  {#if children}{@render children()}{/if}
  {#if onClose}
    <span
      class="-mr-1 p-0.5 text-xs leading-none text-muted opacity-0 transition-all group-hover:opacity-100 hover:text-danger"
      role="button"
      tabindex="0"
      onclick={handleClose}
      onkeydown={(event) => event.key === 'Enter' && handleClose(event)}
    >
      <X size={12} />
    </span>
  {/if}
</button>
