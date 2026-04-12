<script lang="ts">
	import { cn } from '$lib/utils/cn'
	import { Menubar } from 'bits-ui'

	let { class: className, ...rest }: { class?: string } = $props();
</script>

<Menubar.Separator class={cn('h-px bg-edge mx-2 my-1', className)} {...rest} />
