/**
 * CLI provider metadata for session creation cards.
 */

export interface ProviderMeta {
	id: string;
	label: string;
	icon: string;
	textIcon: string;
	textAspect: number;
	gradientFrom: string;
	gradientTo: string;
}

export const allProviders: ProviderMeta[] = [
	{
		id: 'claude_code',
		label: 'Claude Code',
		icon: '/svg/claudecode.svg',
		textIcon: '/svg/claudecode-text.svg',
		textAspect: 91 / 11,
		gradientFrom: '#f29d84',
		gradientTo: '#763724'
	},
	{
		id: 'codex',
		label: 'Codex',
		icon: '/svg/codex.svg',
		textIcon: '/svg/codex-text.svg',
		textAspect: 91 / 24,
		gradientFrom: '#6ee7b7',
		gradientTo: '#065f46'
	},
	{
		id: 'gemini',
		label: 'Gemini CLI',
		icon: '/svg/gemini.svg',
		textIcon: '/svg/gemini-text.svg',
		textAspect: 98 / 24,
		gradientFrom: '#60a5fa',
		gradientTo: '#1e3a5f'
	},
	{
		id: 'opencode',
		label: 'OpenCode',
		icon: '/svg/opencode.svg',
		textIcon: '/svg/opencode-text.svg',
		textAspect: 138 / 24,
		gradientFrom: '#c084fc',
		gradientTo: '#4c1d95'
	}
];
