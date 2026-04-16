<script lang="ts">
  import { buttonVariants } from '$lib/components/ui/button'
  import { BellIcon } from '$lib/icons/lucideExports'
  import {
    getNotifications,
    isNotificationCenterOpen,
    toggleNotificationCenter
  } from '$lib/stores/notifications.svelte'
  import { cn } from '$lib/utils/cn'

  let notifications = $derived(getNotifications())
  let expanded = $derived(isNotificationCenterOpen())
  let hasNotifications = $derived(notifications.length > 0)
</script>

<button
  type="button"
  class={cn(
    buttonVariants({ variant: 'ghost', size: 'icon-sm' }),
    'relative text-muted',
    expanded && 'bg-surface text-fg'
  )}
  aria-label={expanded ? 'Hide notifications' : 'Show notifications'}
  aria-controls="notifications-surface"
  aria-expanded={expanded}
  data-notifications-toggle="true"
  onclick={toggleNotificationCenter}
>
  <BellIcon size={11} />
  {#if hasNotifications}
    <span class="absolute -top-0.5 -right-0.5 h-1.5 w-1.5 rounded-full bg-accent"></span>
  {/if}
</button>
