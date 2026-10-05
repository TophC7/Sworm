<script lang="ts">
  import { cn } from '$lib/utils/cn'
  import { inputVariants } from './input.svelte'
  import type { HTMLTextareaAttributes } from 'svelte/elements'

  let {
    class: className,
    value = $bindable(''),
    ...rest
  }: Omit<HTMLTextareaAttributes, 'value'> & {
    class?: string
    value?: string
  } = $props()
</script>

<textarea
  data-slot="textarea"
  bind:value
  class={cn(inputVariants(), 'resize-none', className)}
  {...rest}
></textarea>
