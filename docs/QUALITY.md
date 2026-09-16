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

### Historical imported-core audit

```sh
node tools/quality/all.mjs --with-core
```

This additionally runs the strict Clippy audit of the frozen base core's fast and
quality features. It currently fails on historical lint debt. The ordinary gate
explicitly reports that this audit was not run; it does not claim imported code is
lint-clean. Resolving that debt requires synchronized recipes/provenance and
rebuilt scanner parity through [the promotion procedure](PROMOTING_CHANGES.md).
No baseline suppressions or relaxed rules turn these diagnostics into a pass.

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

Full Rust/JS checks currently expose existing source diagnostics (including the imported release core); adding tooling
is not a claim that historical experiments satisfy every new lint. Fix findings
with focused behavioral tests. Do not disable strict rules wholesale to hide them.
The Rust audit checks all targets for base-crate fast/quality features and the
research `decoder-sprint` crate, not every experimental recipe. Selected release experiments also need their own build and parity checks.

## Protected scanner history and promotion

The automatic formatter excludes upstream `sources/`, datasets, generated outputs,
benchmark reports/results, release `core/` and provenance, frozen JS hosts, and
research files named in experiment `baseHashes`. Formatting those inputs would
invalidate reproducible patches and WASM hashes. VS Code's native rustfmt runs on
an explicitly edited Rust document: do not casually edit/save frozen inputs.

The research scanner is the classical EAN-13 code in `../barcode/rust/barcode-core/`.
Exact variants are defined by `experiments/*.json` plus `.patch` files and rebuilt
by `experiments/build_guarded.py`. In the research repo, consult `js/camera-demo/src/lib/versions.ts`
and the matching manifest to identify the requested version; do not assume the
base Cargo aliases are the latest or promote a different mode merely because
its name sounds newer. The current four-mode release selection is recorded in `provenance/modes.json`;
see `docs/PROMOTION_DETAIL_20260914.md` for the latest promotion evidence.

When promoting:

1. Select and reproduce the exact experiment in an isolated output directory.
2. Port only required source into this release repository; consult
   `docs/PROMOTING_CHANGES.md`. Preserve public bindings and API improvements.
3. If formatting scanner inputs, format base and patched sources together,
   regenerate patches/manifests/provenance, and create new immutable version tags
   for changed bytes. Never automatically rewrite recorded checksums to hide drift.
4. Run selected native/WASM builds, hash/behavior verification, and cross-language
   parity tests. Keep private images, neural models and unrelated research out.

Sources: [Claude Code hooks](https://code.claude.com/docs/en/hooks),
[Codex hooks](https://learn.chatgpt.com/docs/hooks),
[Prettier editor integration](https://prettier.io/docs/editors).

## Maintained release binding gate

Run `node tools/quality/release.mjs all` (or `rust`, `js`, `python`, `native`).
Set `QUALITY_PYTHON` to the Python environment containing Pillow, NumPy and torch;
set `JAVA_HOME` to a JDK 22+ installation. Install clang/clang++ for C/C++ checks.
The gate checks all four reproduced modes of the Rust facade and C ABI with Clippy
all/pedantic and warnings as errors; TypeScript with strict ESLint and tsc; Python
with Ruff ALL, ty and strict mypy; C/C++ with compiler conversion/sign warnings
as errors; and Java with javac all warnings as errors. Java's restricted native
FFM calls have documented, method-local exceptions; other warnings stay errors.

Historical imported core and JS host remain excluded from the maintained binding
gate, just as they are from Python lint and automatic formatting. Their debt is
not hidden or declared clean: `node tools/quality/check.mjs rust` audits the core
and still fails. A full scanner migration must regenerate recipes/provenance and
validate rebuilt versions before this exception can be removed.
