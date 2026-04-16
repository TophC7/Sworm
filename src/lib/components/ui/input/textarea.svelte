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

<textarea
  bind:value
  class={cn(
    'w-full resize-none rounded border border-edge bg-surface px-2 py-1.5 text-[0.75rem] text-fg outline-none placeholder:text-subtle focus:border-accent/50',
    className
  )}
  {...rest}
></textarea>
