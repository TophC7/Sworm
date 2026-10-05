<script lang="ts">
  import { Alert, AlertTitle, AlertDescription } from '$lib/components/ui/alert'
  import { IconButton } from '$lib/components/ui/button'
  import TabBeam from '$lib/components/ui/tab-beam.svelte'
  import { X } from '$lib/icons/lucideExports'
  import type { Notification, NotificationTone } from '$lib/features/notifications/state.svelte'
  import { cn } from '$lib/utils/cn'

  let {
    notification,
    onDismiss,
    showTimestamp = true
  }: {
    notification: Notification
    onDismiss: (id: string) => void
    showTimestamp?: boolean
  } = $props()

  function getNotificationAlertVariant(tone: NotificationTone) {
    return tone === 'neutral' ? 'info' : tone === 'error' ? 'danger' : tone
  }

  function formatNotificationTimestamp(timestamp: number): string {
    const diff = Date.now() - timestamp
    if (diff < 60_000) return 'just now'
    if (diff < 3_600_000) return `${Math.floor(diff / 60_000)}m ago`
    if (diff < 86_400_000) return `${Math.floor(diff / 3_600_000)}h ago`
    return new Date(timestamp).toLocaleDateString()
  }
</script>

<Alert variant={getNotificationAlertVariant(notification.tone)} class="group min-h-[4.25rem] pr-10">
  <div class="min-w-0 flex-1 space-y-1">
    <AlertTitle class="pr-1">{notification.title}</AlertTitle>
    {#if notification.description}
      <AlertDescription class="pr-1">{notification.description}</AlertDescription>
    {/if}

    {#if notification.loading}
      <div class="relative mt-1 h-1.5 overflow-hidden rounded-full bg-ground/70" aria-hidden="true">
        <TabBeam class="h-full rounded-full" />
      </div>
    {/if}

    <div class={cn('text-2xs leading-none text-subtle', !showTimestamp && 'invisible')} aria-hidden={!showTimestamp}>
      {formatNotificationTimestamp(notification.timestamp)}
    </div>
  </div>

  <IconButton tooltip="Dismiss" class="absolute top-2 right-2" onclick={() => onDismiss(notification.id)}>
    <X size={10} />
  </IconButton>
</Alert>
