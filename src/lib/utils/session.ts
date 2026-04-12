/**
 * Session status display helpers.
 *
 * Shared by SecondaryTabBar, PaneTabBar, and TerminalSessionManager.
 */

import { allProviders, directOptions } from '$lib/data/providers';

export function statusIcon(status: string): string {
	if (status === 'running') return '\u25CF';
	if (status === 'stopped' || status === 'exited') return '\u25CB';
	if (status === 'failed') return '\u2715';
	return '\u25CC';
}

export function statusColorClass(status: string): string {
	if (status === 'running') return 'text-success';
	if (status === 'failed') return 'text-danger';
	return 'text-muted';
}

/** Derive label from the canonical provider lists. */
export function providerLabel(providerId: string): string {
	return allProviders.find((p) => p.id === providerId)?.label
		?? directOptions.find((p) => p.id === providerId)?.label
		?? providerId;
}
