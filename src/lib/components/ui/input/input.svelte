<script lang="ts">
  import { cn } from '$lib/utils/cn'

  let {
    class: className,
    value = $bindable(''),
    ...rest
  }: {
    class?: string
    value?: string | number
    [key: string]: unknown
  } = $props()
</script>

<input
  bind:value
  class={cn(
    'w-full rounded border border-edge bg-surface px-2.5 py-1.5 text-base text-fg transition-colors outline-none placeholder:text-subtle focus:border-accent',
    className
  )}
  {...rest}
/>
