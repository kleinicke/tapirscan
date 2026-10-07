# Repository boundaries

This repository contains the selected scanner, supported bindings, tests, build
tools and documentation. It must build and test without a
research checkout.

Turbo presets and Low Classic are kept on purpose, including their source and
build settings, and the demo's Turbo entries. An experimental name alone is not
evidence that code is unused.

Research belongs in a separate experiment workspace: hypotheses, exploratory
commands, decoder-only adapters, alternative algorithms, failed variants and
reports. Scanner experiments edit the real source in isolated worktrees; they do
not maintain a second production implementation.

Integration brings across only the selected implementation, regression tests,
and documentation. Remove unused branches
and exploratory switches first. Runtime diagnostics that support the scanner
are product code.
