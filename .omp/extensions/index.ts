import type { ExtensionAPI } from "@oh-my-pi/pi-coding-agent";

export default function swormHooks(pi: ExtensionAPI): void {
	// 1. Enforce Bun: block npm, pnpm, node commands
	pi.on("tool_call", async (event) => {
		if (event.toolName !== "bash") return;
		const cmd = String(event.input?.command ?? "");
		if (/^\s*(npm|npx|pnpm|pnpx|node)\b/.test(cmd)) {
			return {
				block: true,
				reason: "Blocked: use bun instead of npm/pnpm/node",
			};
		}
	});

	// 2. Pre-commit check: auto-format staged files before git commit executes
	pi.on("tool_call", async (event) => {
		if (event.toolName !== "bash") return;
		const cmd = String(event.input?.command ?? "");
		if (!/\bgit\s+commit\b/.test(cmd)) return;

		try {
			const stagedRes = await pi.exec("git", [
				"diff",
				"--cached",
				"--name-only",
				"--diff-filter=ACMR",
			]);
			if (stagedRes.code !== 0 || !stagedRes.stdout.trim()) return;

			const staged = stagedRes.stdout
				.split("\n")
				.map((s) => s.trim())
				.filter(Boolean);

			const frontendFiles = staged.filter((f) =>
				/\.(ts|js|svelte|css|json|html)$/.test(f),
			);
			if (frontendFiles.length > 0) {
				await pi.exec("bun", [
					"prettier",
					"--write",
					"--ignore-unknown",
					...frontendFiles,
				]);
				await pi.exec("git", ["add", ...frontendFiles]);
			}

			const rustFiles = staged.filter((f) => /\.rs$/.test(f));
			if (rustFiles.length > 0) {
				await pi.exec("cargo", ["fmt", "--all"]);
				await pi.exec("git", ["add", ...rustFiles]);
			}
		} catch {
			// Continue execution on format failure
		}
	});

	// 3. Post-edit formatting: run prettier on edited/written frontend files
	pi.on("tool_result", async (event) => {
		if (event.isError) return;

		const filesToFormat = new Set<string>();

		if (event.toolName === "write") {
			const path = String(event.input?.path ?? "");
			if (/\.(ts|svelte|js|css)$/.test(path)) {
				filesToFormat.add(path);
			}
		} else if (event.toolName === "edit") {
			const rawInput = String(event.input?.input ?? "");
			const matches = rawInput.matchAll(/^\[([^#\]\n]+)(?:#[0-9A-Fa-f]+)?\]/gm);
			for (const match of matches) {
				const file = match[1].trim();
				if (/\.(ts|svelte|js|css)$/.test(file)) {
					filesToFormat.add(file);
				}
			}
		}

		for (const file of filesToFormat) {
			try {
				await pi.exec("bun", ["prettier", "--write", file]);
			} catch {
				// Non-blocking formatting failure
			}
		}
	});
}
