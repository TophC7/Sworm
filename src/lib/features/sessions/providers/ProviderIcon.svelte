<!--
  @component
  ProviderIcon — a provider's brand mark image, or its Lucide icon in the current text color.
-->

<script lang="ts">
  import type { ProviderIconSource } from '$lib/features/sessions/providers/catalog'
  import { cn } from '$lib/utils/cn'

  let {
    icon,
    size,
    alt = '',
    class: className
  }: { icon: ProviderIconSource; size: number; alt?: string; class?: string } = $props()
</script>

{#if typeof icon === 'string'}
  <img src={icon} {alt} width={size} height={size} class={cn('shrink-0', className)} />
{:else}
  {@const Icon = icon}
  <Icon {size} class={cn('shrink-0', className)} aria-label={alt || undefined} aria-hidden={alt ? undefined : true} />
{/if}
