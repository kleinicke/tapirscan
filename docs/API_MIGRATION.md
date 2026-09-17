# Migration to the uniform API

These are intentional breaking changes in version 1.2.0, an explicit early-library
exception to the compatibility policy. Do not
publish them as a compatible 1.1.x update. Engine recipes and native ABI 4 are
unchanged; this migration changes the application interfaces.

| Previous                                               | Replacement                                        |
| ------------------------------------------------------ | -------------------------------------------------- |
| Python `finish_candidates=False` / `True`              | `extended_budget=False` / `True`                   |
| JavaScript `finishCandidates: false` / `true`          | `extendedBudget: false` / `true`                   |
| Rust `finish_candidates: false` / `true`               | `extended_budget: false` / `true`                  |
| Rust `scan_all(image)`                                 | `scan(image)?.barcodes`                            |
| Rust `scan_one(image)`                                 | Scan, then select a barcode from the result        |
| Rust `scan_detailed(image, options)`                   | `scan_with_options(image, options)`                |
| Debug-only undecoded geometry                          | `result.undecoded`, always available               |
| Rust `barcode.metadata.field`                          | `barcode.field`, matching the other bindings       |
| Rust false for unavailable GS1/initialization metadata | `None`; known flags use `Some(bool)`               |
| Rust tight floating-point `rect()`                     | Enclosing integer pixel bounds, matching Python/JS |

The Rust changes apply to free functions and reusable scanner methods. Selection
by highest support still scans all selected candidates; it is not a cross-format
confidence estimate. There are no deprecated aliases for the replaced APIs.

A proposal without an accepted decode is called **undecoded**, not unreadable.
It could be a false candidate or need work the selected policy did not perform.
`unfinished` reports limits and deferrals, not a count of missed barcodes.

The extended-budget flag is valid for all formats and requests additional reader
work. Exact budgets and stages are implementation details that may evolve. Today
it relaxes shared EAN/UPC retries; other readers keep their current budgets. False
preserves the default decoding policy. Undecoded geometry is retained on normal
scans, which may add result-conversion cost; no speed improvement is claimed.
Raw traces remain opt-in through `debug`.

The intermediate `candidate_budget` / `candidateBudget` API and `CandidateBudget`
enum are removed. Replace `"shared"` with false and `"per_candidate"` with true
using `extended_budget` / `extendedBudget`. There is no EAN/UPC format restriction.

Rust `scan(image)` now supplies default scan options automatically. Use
`scan_with_options(image, options)` for overrides on free functions or scanners.
The support-based convenience selection is named `best` in all three APIs.
