# Shared formatting and checks

## One-command quality check

```sh
node tools/quality/all.mjs
```

This is also the CI quality gate on macOS and Linux. It checks repository-wide
formatting (excluding protected snapshots), import provenance, maintained Rust
and C ABI bindings in all four modes, JS/TS lint and package/consumer types, Python
lint and types, C/C++/Java compiler warnings, Svelte diagnostics and quality-tool
regression tests. It continues after failures, prints a final summary, and exits
nonzero if any stage fails. Each run saves full logs in `.quality-cache/check-*/`;
CI uploads them on failure. Checks do not rewrite source files.

Runtime tests, package installation tests and scanner parity remain separate CI
steps; a passing static gate does not replace them.

### Local prerequisites

Run `node tools/quality/install.mjs` and install the binding and demo dependencies
as described in [development](DEVELOPMENT.md). Use a Python 3.10–3.12 environment
with `Pillow`, `numpy==2.2.6`, `torch`, `zxing-cpp` and `typing_extensions` installed.
NumPy's version matches CI and provides stubs compatible with the Python 3.10 API
target. Set `QUALITY_PYTHON` to that interpreter and `JAVA_HOME` to a JDK 22+.

Alternatively, save your machine's paths once in the Git-ignored file
`.quality-tools/environment.json`:

```json
{
  "QUALITY_PYTHON": "/absolute/path/to/python-environment/bin/python",
  "JAVA_HOME": "/absolute/path/to/jdk"
}
```

Explicit environment variables override this file. Without a Python setting,
the gate resolves `python3` on PATH to its executable path before calling ty.
Missing dependencies or a missing JDK fail checks; they are never silently skipped.

### Imported scanner checks

The ordinary quality gate includes strict Clippy checks for the maintained core in all four
production modes, and the multiformat crate.
Historical snapshots remain excluded from automatic formatting. Production
source changes use ordinary formatting and require recorded provenance and [promotion validation](PROMOTING_CHANGES.md).

## Tools and focused checks

The release and sibling `../barcode` research repositories share pinned
Rust 1.91.1 rustfmt/Clippy and the exact JS tooling lockfile in `tools/quality/`.
Prettier formats JS/TS/Svelte and ordinary JSON/CSS/Markdown/YAML. ESLint checks JS,
with typescript-eslint strict type-aware rules for TS; `tsc` checks types. Retain
the existing `svelte-check` command in each Svelte app. Ruff formats Python.

Use Node 24 (the exact version is in `.nvmrc`), Python 3 and rustup.
Install on a new checkout:

```sh
node tools/quality/install.mjs
```

This installs repo-local tools, synchronizes Claude Code/Codex hooks and installs
an automatic staged-format Git hook, preserving an existing custom hooks path.
VS Code settings enable format-on-save; install the recommended extensions.
Restart agent sessions after changing hook configuration if they have not reloaded it.
No Cursor configuration or background watcher is used.

Agents format once after a coherent batch of edits, before relevant tests/checks.
Re-read only affected sections if another edit is needed after formatting; do not
reload whole files merely because a formatter ran. Do not run full research JS
lint for Rust-only work. Run the checks relevant to changed code and report failures.

Claude Code and Codex `UserPromptSubmit` hooks record starting file hashes once.
The read-only `Stop` hook checks formatting only for files changed during the turn.
It does not rewrite files and requests at most one continuation to fix omissions,
preventing repeated hook loops. An explicit command and the staged Git check remain
necessary if a hook was unavailable or its final continuation still has failures.
Editor format-on-save remains enabled. There are no per-edit formatting hooks or
background writers. Do not have two agents edit the same file concurrently.

```sh
node tools/quality/cli.mjs format path/to/edited-file.rs
node tools/quality/cli.mjs check-format path/to/edited-file.ts
node tools/quality/cli.mjs lint path/to/edited-file.ts
node tools/quality/check.mjs rust
node tools/quality/check.mjs js
```

With no file arguments, format/check-format use tracked changes against HEAD.
Use explicit files for untracked work. `--all` is an explicit repository-wide
operation; automatic hooks never reformat the whole tree. Lint/check commands are
read-only; no ESLint/Clippy semantic fixes run automatically. The Git hook formats
staged content and updates the index atomically. It never
stages working-tree content: fully staged files are aligned with the formatted
index, while partially staged working files remain untouched. Those files may
show formatting differences in their unstaged diff. Syntax errors or missing
tools still stop the commit; formatting-only differences are fixed automatically.
Path-only commits (`git commit --only`) use a temporary index; Git can leave
formatting differences in the regular index afterward. Prefer committing the
staged selection normally. Immutable camera-demo vendor hosts are excluded.

Fix lint findings with focused behavioral tests. Do not disable strict rules wholesale.
The release Rust audit checks all targets for all four production core modes and the multiformat crate. Research-only recipes are
outside that gate. Runtime parity is verified separately.

## Protected scanner history and promotion

The automatic formatter excludes upstream `sources/`, datasets, generated outputs,
benchmark reports/results, `historical/` and provenance, frozen JS hosts, and
research files named in experiment `baseHashes`. Formatting those inputs would
invalidate reproducible patches and WASM hashes. VS Code's native rustfmt runs on
an explicitly edited Rust document: do not casually edit/save frozen inputs.

Production algorithms are maintained directly in `core/src`, with four explicit
`mode-*` features. The formatter includes them. `historical/core` preserves the
old base, feature graph and recipe patches; `scripts/build.py MODE --historical`
reproduces that frozen selection. See [core architecture](../core/README.md).

For a production change, format the coherent edit batch, validate all affected
modes and bindings, then record a new source/WASM identity before integration.
The development build verifies frozen history without requiring the working
source to equal the previous release snapshot. The full quality gate still runs
strict `verify_import.py` against the selected release revision. Follow
[PROMOTING_CHANGES.md](PROMOTING_CHANGES.md); never rewrite frozen history.

Sources: [Claude Code hooks](https://code.claude.com/docs/en/hooks),
[Codex hooks](https://learn.chatgpt.com/docs/hooks),
[Prettier editor integration](https://prettier.io/docs/editors).

## Maintained release binding gate

Run `node tools/quality/release.mjs all` (or `rust`, `js`, `python`, `native`).
Set `QUALITY_PYTHON` to the Python environment containing Pillow, NumPy and torch;
set `JAVA_HOME` to a JDK 22+ installation. Install clang/clang++ for C/C++ checks.
The gate checks all four compiled modes of the Rust facade and C ABI with Clippy
all/pedantic and warnings as errors; TypeScript with strict ESLint and tsc; Python
with Ruff ALL, ty and strict mypy; C/C++ with compiler conversion/sign warnings
as errors; and Java with javac all warnings as errors. Java's restricted native
FFM calls have documented, method-local exceptions; other warnings stay errors.

Frozen JS hosts remain excluded from maintained-source lint and automatic formatting.
Maintained core and imported multiformat Rust are checked by the regular gate;
scanner parity and package installation checks remain separate runtime checks.
