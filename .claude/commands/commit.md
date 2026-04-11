# Commit

Create a conventional commit from currently staged changes.

## Steps

1. Run `git diff --cached --stat` and `git diff --cached` to see what's staged. If nothing is staged, tell the user and stop.

2. Analyze the staged diff and draft a conventional commit message:
   - First line: `type(scope): summary` — summarize the main intent(s), keep under 72 chars
   - Types: `feat`, `fix`, `refactor`, `chore`, `docs`, `style`, `test`, `perf`, `ci`, `build`
   - Scope: optional, use the most relevant area (e.g. module name, feature)
   - If there are multiple notable changes, add a blank line then bullet points for the important ones
   - Do NOT include Co-Authored-By or any trailers

3. Create the commit using a HEREDOC:
   ```
   git commit -m "$(cat <<'EOF'
   type(scope): summary line

   - important detail 1
   - important detail 2
   EOF
   )"
   ```

4. **If the commit fails for ANY reason** (pre-commit hook, lint, etc.):
   - Do NOT retry, amend, fix files, or take any further action
   - Display the error output
   - Display the exact commit message you attempted to use
   - Stop and let the user decide what to do

5. If the commit succeeds, show the commit hash and summary. Keep it brief.
