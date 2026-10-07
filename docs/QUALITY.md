# Shared formatting and checks

## Quality gate

```sh
node tools/quality/install.mjs   # once per checkout
node tools/quality/all.mjs
```

`all.mjs` is also the CI quality gate on macOS and Linux. It checks repository-wide
formatting, the repository boundary (`scripts/verify_sources.py`), the maintained
binding gates below, the core and multiformat Rust crates in all four modes, and the
quality-tool tests. It continues after failures, prints a summary, and exits nonzero
if any stage fails. Full logs go to `.quality-cache/check-*/`; checks never rewrite
source. Runtime tests, package installation tests and scanner parity are separate
CI steps (see [validation](VALIDATION.md)).

`node tools/quality/release.mjs all` (or `rust`, `js`, `python`, `native`) runs only
the binding checks, without the formatting, source, core-crate and tool-test stages
that `all.mjs` adds. It runs Clippy (all and pedantic, warnings as errors) on all
four modes of the Rust facade and C ABI; strict ESLint and `tsc` for TypeScript; Ruff,
ty and strict mypy for Python; clang with conversion and sign warnings as errors for
C/C++; and `javac -Xlint:all -Werror` for Java (the restricted FFM calls have documented,
method-local exceptions).

## Environment

- Rust 1.91.1 (rustfmt and Clippy), Node 24 (`.nvmrc`), Python 3.10-3.12, clang/clang++, a JDK 22+.
- `QUALITY_PYTHON`: a Python environment with `Pillow`, `torch`, `zxing-cpp`,
  `typing_extensions` and the NumPy version CI uses (see `.github/workflows/ci.yml`;
  it provides stubs compatible with the Python 3.10 API target). Without it, `python3`
  from PATH is used.
- `JAVA_HOME`: the JDK for the Java checks.
- Instead of environment variables, save both once in the Git-ignored
  `.quality-tools/environment.json`; explicit variables override it:

```json
{
  "QUALITY_PYTHON": "/absolute/path/to/python-environment/bin/python",
  "JAVA_HOME": "/absolute/path/to/jdk"
}
```

Missing dependencies or a missing JDK fail the checks; nothing is skipped silently.

## Formatting and focused checks

Prettier formats JS/TS/Svelte and ordinary JSON/CSS/Markdown/YAML, Ruff formats
Python, and rustfmt formats Rust. `install.mjs` installs the repo-local tools and a
Git hook that formats staged content; VS Code settings enable format-on-save.
Claude Code and Codex hooks configured by `install.mjs` check formatting at the end
of a turn; restart agent sessions after changing them. Format once after a coherent
batch of edits, before running checks.

```sh
node tools/quality/cli.mjs format path/to/edited-file.rs
node tools/quality/cli.mjs check-format path/to/edited-file.ts
node tools/quality/cli.mjs lint path/to/edited-file.ts
node tools/quality/check.mjs rust
node tools/quality/check.mjs js
```

Without file arguments, `format` and `check-format` use tracked changes against HEAD;
pass explicit files for untracked work and `--all` for the whole repository.
Lint and check commands are read-only. Frozen vendor files and generated assets are
excluded from formatting. Fix lint findings with focused behavioral tests rather than
disabling strict rules wholesale.
