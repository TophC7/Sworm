<script lang="ts">
  import { Alert, AlertTitle, AlertDescription } from '$lib/components/ui/alert'
  import { Button, buttonVariants, IconButton } from '$lib/components/ui/button'
  import { ButtonGroup } from '$lib/components/ui/button-group'
  import {
    DropdownMenuContent,
    DropdownMenuItem,
    DropdownMenuRoot,
    DropdownMenuTrigger
  } from '$lib/components/ui/dropdown-menu'
  import NotificationProgressBar from '$lib/features/notifications/NotificationProgressBar.svelte'
  import {
    formatNotificationTimestamp,
    getNotificationAlertVariant,
    getNotificationProgressVariant
  } from '$lib/features/notifications/notificationUi'
  import { ChevronDown, X } from '$lib/icons/lucideExports'
  import type {
    Notification,
    NotificationMenuAction,
    NotificationPrimaryAction
  } from '$lib/features/notifications/state.svelte'
  import { cn } from '$lib/utils/cn'

  let {
    notification,
    onDismiss,
    showTimestamp = true,
    class: className
  }: {
    notification: Notification
    onDismiss: (id: string) => void
    showTimestamp?: boolean
    class?: string
  } = $props()

  let hasActions = $derived(!!notification.primaryAction || !!notification.secondaryAction)

  function isMenuAction(action?: NotificationPrimaryAction): action is NotificationMenuAction {
    return action != null && 'kind' in action && action.kind === 'menu'
  }
</script>

<Alert variant={getNotificationAlertVariant(notification.tone)} class={cn('group min-h-[4.25rem] pr-10', className)}>
  <div class="min-w-0 flex-1 space-y-1">
    <AlertTitle class="pr-1">{notification.title}</AlertTitle>
    {#if notification.description}
      <AlertDescription class="pr-1">{notification.description}</AlertDescription>
    {/if}

    {#if notification.loading}
      <NotificationProgressBar
        progress={notification.progress}
        variant={getNotificationProgressVariant(notification.tone)}
        class="mt-1"
      />
    {/if}

    {#if hasActions}
      <div class="flex flex-wrap items-center gap-1.5 pt-0.5">
        {#if notification.primaryAction}
          {#if isMenuAction(notification.primaryAction)}
            <ButtonGroup>
              <Button
                variant="default"
                size="xs"
                class="h-6 rounded"
                disabled={notification.primaryAction.disabled}
                onclick={notification.primaryAction.onSelect}
              >
                {notification.primaryAction.label}
              </Button>

              <DropdownMenuRoot>
                <DropdownMenuTrigger
                  class={cn(buttonVariants({ variant: 'default', size: 'xs' }), 'rounded px-1 py-1 text-muted')}
                  disabled={notification.primaryAction.disabled}
                >
                  <ChevronDown size={11} />
                </DropdownMenuTrigger>
                <DropdownMenuContent class="notifications-surface-interactive min-w-[180px] text-sm">
                  {#each notification.primaryAction.items as action}
                    <DropdownMenuItem
                      class={action.disabled ? 'pointer-events-none opacity-50' : ''}
                      destructive={action.destructive}
                      onclick={() => {
                        if (action.disabled) return
                        action.onSelect()
                      }}
                    >
                      {action.label}
                    </DropdownMenuItem>
                  {/each}
                </DropdownMenuContent>
              </DropdownMenuRoot>
            </ButtonGroup>
          {:else}
            <Button
              variant={notification.primaryAction.destructive ? 'destructive' : 'default'}
              size="xs"
              class="h-6"
              disabled={notification.primaryAction.disabled}
              onclick={notification.primaryAction.onSelect}
            >
              {notification.primaryAction.label}
            </Button>
          {/if}
        {/if}

        {#if notification.secondaryAction}
          <Button
            variant={notification.secondaryAction.destructive ? 'destructive' : 'ghost'}
            size="xs"
            class="h-6"
            disabled={notification.secondaryAction.disabled}
            onclick={notification.secondaryAction.onSelect}
          >
            {notification.secondaryAction.label}
          </Button>
        {/if}
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
