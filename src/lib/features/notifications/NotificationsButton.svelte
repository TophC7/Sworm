<script lang="ts">
  import { StatusChip } from '$lib/components/ui/status-chip'
  import { BellIcon } from '$lib/icons/lucideExports'
  import {
    getNotifications,
    isNotificationCenterOpen,
    toggleNotificationCenter
  } from '$lib/features/notifications/state.svelte'

  let notifications = $derived(getNotifications())
  let expanded = $derived(isNotificationCenterOpen())
  let hasNotifications = $derived(notifications.length > 0)
</script>

<StatusChip
  shape="circle"
  active={expanded}
  class="relative"
  aria-label={expanded ? 'Hide notifications' : 'Show notifications'}
  aria-controls="notifications-surface"
  aria-expanded={expanded}
  data-notifications-toggle="true"
  onclick={toggleNotificationCenter}
>
  <BellIcon size={11} />
  {#if hasNotifications}
    <span class="absolute -top-0.5 -right-0.5 size-1.5 rounded-full bg-accent"></span>
  {/if}
</StatusChip>
