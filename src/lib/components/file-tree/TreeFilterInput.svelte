<script lang="ts">
  import { Input } from '$lib/components/ui/input'

  let {
    value = $bindable(''),
    placeholder,
    ariaLabel
  }: {
    value?: string
    placeholder: string
    ariaLabel: string
  } = $props()
</script>

<div class="border-b border-edge px-2 py-1.5">
  <Input
    bind:value
    {placeholder}
    aria-label={ariaLabel}
    class="h-7 py-1 text-sm"
    onkeydown={(e: KeyboardEvent) => {
      if (e.key === 'Escape' && value.length > 0) {
        e.preventDefault()
        value = ''
      }
    }}
  />
</div>
