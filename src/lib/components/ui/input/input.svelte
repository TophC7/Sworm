<script lang="ts">
  import { cn } from '$lib/utils/cn'

  let {
    class: className,
    value = $bindable(''),
    ...rest
  }: {
    class?: string
    value?: string
    [key: string]: unknown
  } = $props()
</script>

<input
  bind:value
  class={cn(
    'w-full rounded border border-edge bg-surface px-2 py-1.5 text-sm text-fg outline-none placeholder:text-subtle focus:border-accent',
    className
  )}
  {...rest}
/>
