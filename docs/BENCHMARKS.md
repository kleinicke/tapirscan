# Evaluating barcode readers

This page holds the rules for fair comparisons. Performance numbers that are
recorded or published also follow the [runtime rule](../AGENTS.md#required-runtime-for-recorded-performance-numbers).

Start with the images your application actually receives. Evaluate decoded
values and complete-frame success together with runtime; a fast incorrect result
is not a successful scan.

## A fair paired comparison

1. Freeze the input rasters. Give every reader identical pixels and dimensions;
   record resizing, orientation correction, and color conversion.
2. Select the same formats for every reader. Record every
   reader option, version, and Tapirscan effort mode. Identify Tapirscan by its
   package version or git commit and the source digest in `wasm/build.json`.
3. Separate cold initialization from warmed scans. Include the same preprocessing
   and result-conversion stages in each timed measurement. Keep individual reader
   timings separate from the demo's total sequential batch time.
4. Use independently reviewed ground truth. Count physical barcode instances,
   including different symbols with equal values. Report unknown/unlabelled cases
   separately rather than assuming every unmatched result is wrong.
5. Report correct instances, missed instances, wrong values, duplicates, and frames
   where every labelled barcode was read. Compare polygons separately from values;
   ZBar's returned points are not equivalent to a full symbol boundary.
6. Report median, p90/p95, and the number of images—not just a mean or a best case.
   Include devices, browser/runtime versions, warmups, repetitions and exclusions.
7. Keep development images separate from the final holdout. Rotations or crops of
   one photograph are correlated samples, not independent captures.

For camera applications, also measure time to the first correct result and the
rate of incorrect results over a sequence. A single-image benchmark cannot
establish autofocus behavior, motion tolerance, or battery use.

## What the repository validates

The repository checks ([validation](VALIDATION.md)) cover reproducible WASM builds,
native/WASM output parity, generated small-barcode and rotation tests, and browser
parity. These establish integration parity; synthetic regression tests and timing
during builds or CI are **not** an independent accuracy benchmark or
evidence that Tapirscan always beats ZXing/ZBar. Numbers from any cohort,
including your own, describe that cohort and runtime only. Performance claims
should link to a reproducible report that follows the protocol above.
