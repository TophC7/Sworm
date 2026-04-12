<script lang="ts">
  import { Menubar } from 'bits-ui'
  import { cn } from '$lib/utils/cn'
  import type { Snippet } from 'svelte'

  let {
    class: className,
    children,
    ...rest
  }: {
    class?: string
    children?: Snippet
  } = $props()
</script>

<Menubar.SubContent
  class={cn(
    'z-50 min-w-[160px] rounded-lg border border-edge bg-raised py-1 text-[0.8rem] shadow-[0_8px_24px_rgba(0,0,0,0.4)]',
    className
  )}
  {...rest}
>
  {#if children}{@render children()}{/if}
</Menubar.SubContent>
