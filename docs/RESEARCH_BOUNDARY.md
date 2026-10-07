# Production and research boundary

The public main branch contains the selected scanner, supported bindings, tests,
build tools, current documentation and release provenance. It must build and test
without a sibling research repository or frozen research implementation.

Turbo variants and Low Classic are deliberately retained, including their demo
entries, source, build settings and pinned artifacts. Numbered Turbo presets are available through the experimental JavaScript API. An experimental name alone is not evidence that code is unused.

Research belongs in the separate experiment workspace: hypotheses, exploratory
commands, decoder-only adapters, alternative algorithms, failed variants, dated
reports and retained observations. Scanner experiments edit the real source in
isolated worktrees; they do not maintain a second production implementation.

Promotion brings across only the selected implementation, regression tests,
current user/developer documentation and immutable source/artifact identities.
Remove unused branches and exploratory switches before integrating. Runtime
diagnostics used to support the selected scanner remain legitimate product code.

The research archive preserves the pre-cleanup commit
`7db02322a50bd7580d7389a3af0186d773536b10`, original paths and content hashes under
experiment `production-research-boundary-20260928`. Git history remains intact;
this cleanup changes the current tree, not earlier commits. No release build
resolves files through that archive.
