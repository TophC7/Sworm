// Provider state module using Svelte 5 runes.

import { backend } from '$lib/api/backend';
import type { ProviderStatus } from '$lib/types/backend';

let providers = $state<ProviderStatus[]>([]);
let loading = $state(false);

export function getProviders() {
	return providers;
}

export function getProvidersLoading() {
	return loading;
}

export async function loadProviders() {
	loading = true;
	try {
		providers = await backend.providers.list();
	} catch (e) {
		console.error('Failed to load providers:', e);
	} finally {
		loading = false;
	}
}

export async function refreshProviders() {
	loading = true;
	try {
		providers = await backend.providers.refresh();
	} catch (e) {
		console.error('Failed to refresh providers:', e);
	} finally {
		loading = false;
	}
}

export async function loadProvidersForProject(projectId: string) {
	loading = true;
	try {
		providers = await backend.providers.listForProject(projectId);
	} catch {
		// Fall back to global detection
		providers = await backend.providers.list();
	} finally {
		loading = false;
	}
}

export function getConnectedProviders() {
	return providers.filter((p) => p.status === 'connected');
}
