# Evaluating barcode readers

Start with the images your application actually receives. Evaluate decoded
values and complete-frame success together with runtime; a fast incorrect result
is not a successful scan.

## A fair paired comparison

1. Freeze the input rasters. Give every reader identical pixels and dimensions;
   record resizing, orientation correction, and color conversion.
2. Select the same formats. The public demo compares EAN-13 only. Record every
   reader option, version, WASM hash, and Tapirscan effort mode.
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

## What has been validated here

The release contains reproducible source hashes, native/WASM output checks,
generated small-barcode and rotation tests, and a browser comparison against the
promoted research pipeline. These establish integration parity. They are **not**
a new independent accuracy benchmark or evidence that Tapirscan always beats
ZXing/ZBar.

Detailed development studies remain in the research repository. Promotion records
summarize the relevant limitations and exact selections; start with
[the current promotion](PROMOTION_DETAIL_20260914.md). Performance claims in future
releases should link to a reproducible public report with the protocol above.

## Reporting a difficult image

Include the input dimensions, effort mode, selected formats, runtime and expected
value if known. Share an image only if you have permission to publish it; remove
personal or confidential content first. A small reproducible example is more
useful than a screenshot of an overlay alone.
