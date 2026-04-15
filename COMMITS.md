# Commit Convention

This project uses [Conventional Commits](https://www.conventionalcommits.org/) for clear and consistent commit messages.

## Format

```
<type>(<scope>): <description>
```

## Types

- `feat:` - New feature
- `fix:` - Bug fix
- `docs:` - Documentation changes
- `style:` - Code formatting (no functional changes)
- `refactor:` - Code restructuring (no functional changes)
- `test:` - Test additions or fixes
- `chore:` - Maintenance tasks, dependency updates
- `perf:` - Performance improvements
- `ci:` - CI/CD changes
- `build:` - Build system or dependencies

## Examples

```
feat: add user authentication
fix(api): handle null response correctly
docs: update API documentation
chore: upgrade dependencies
```

## Breaking Changes

Add `!` after type for breaking changes: `feat!: change API response format`