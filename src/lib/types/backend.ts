// Typed interfaces for the Rust backend IPC responses.
// Keep in sync with models in src-tauri/src/models/ and commands/.

export interface AppInfo {
	name: string;
	version: string;
}

export interface EnvProbeResult {
	detected_shell: string;
	base_path: string;
	shell_path: string | null;
	merged_path: string;
	probe_succeeded: boolean;
	gdk_backend: string | null;
	webkit_disable_dmabuf_renderer: string | null;
	webkit_disable_compositing_mode: string | null;
}

export interface PtyEvent {
	type: 'started' | 'exit' | 'error';
	session_id: string;
	pid?: number | null;
	code?: number | null;
	message?: string;
}

export interface Project {
	id: string;
	name: string;
	path: string;
	default_branch: string | null;
	base_ref: string | null;
	created_at: string;
	updated_at: string;
}

export interface Session {
	id: string;
	project_id: string;
	provider_id: string;
	title: string;
	cwd: string;
	branch: string | null;
	status: SessionStatus;
	shared_workspace: boolean;
	auto_approve: boolean;
	provider_resume_token: string | null;
	created_at: string;
	updated_at: string;
	last_started_at: string | null;
	last_stopped_at: string | null;
}

export type SessionStatus = 'idle' | 'starting' | 'running' | 'stopped' | 'exited' | 'failed';

export type ProviderConnectionStatus = 'connected' | 'missing' | 'error';

export interface ProviderStatus {
	id: string;
	label: string;
	status: ProviderConnectionStatus;
	version: string | null;
	resolved_path: string | null;
	message: string | null;
	install_hint: string;
}

export interface GeneralSettings {
	theme: string;
	terminal_font_family: string;
	terminal_font_size: number;
}

export interface ProviderConfig {
	provider_id: string;
	enabled: boolean;
	binary_path_override: string | null;
	extra_args: string[];
}

export interface ProviderSettingsEntry {
	provider: ProviderStatus;
	config: ProviderConfig;
}

export interface SettingsPayload {
	general: GeneralSettings;
	providers: ProviderSettingsEntry[];
}

export interface GitChange {
	path: string;
	status: string;
	staged: boolean;
	additions: number | null;
	deletions: number | null;
}

export interface GitSummary {
	is_repo: boolean;
	branch: string | null;
	base_ref: string | null;
	ahead: number | null;
	behind: number | null;
	changes: GitChange[];
	staged_count: number;
	unstaged_count: number;
	untracked_count: number;
}

export interface GitCommit {
	hash: string;
	short_hash: string;
	author: string;
	date: string;
	message: string;
}

export interface DiffContext {
	raw_diff: string;
	old_content: string | null;
	new_content: string | null;
}
