<script lang="ts">
	import { DropdownMenu } from 'bits-ui';
	import { cn } from '$lib/utils/cn';

	let { class: className, ...rest }: { class?: string } = $props();
</script>

<DropdownMenu.Separator class={cn('h-px bg-edge mx-2 my-1', className)} {...rest} />
