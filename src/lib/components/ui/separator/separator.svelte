<script lang="ts">
  import { Separator as SeparatorPrimitive } from 'bits-ui'
  import { cn } from '$lib/utils/cn'

  let {
    orientation = 'horizontal',
    class: className,
    ...rest
  }: {
    orientation?: 'horizontal' | 'vertical'
    class?: string
  } = $props()
</script>

<SeparatorPrimitive.Root
  {orientation}
  class={cn('shrink-0 bg-edge', orientation === 'horizontal' ? 'h-px w-full' : 'h-full w-px', className)}
  {...rest}
/>
