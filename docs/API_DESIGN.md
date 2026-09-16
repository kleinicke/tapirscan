# Python and JavaScript API design

The 1.1.0 APIs use one-shot `scan(image)` and reusable `Scanner` entry points.
Both return a consistent result containing all decoded instances, source-image
geometry, effort, timing and incomplete-work status. `.best` selects the
highest-support read; selection does not change decoding work. Support is reader-specific,
so this is not a cross-format reliability comparison. Empty results
remain valid results. Python results are iterable sequences; JavaScript exposes
`result.barcodes`. Results survive scanner disposal and are immutable. Python's
`.values` returns a fresh list; JavaScript's `.values` is a frozen array.

## Options and inputs

See the complete option tables in the [Python](../bindings/python/README.md) and
[JavaScript](../bindings/javascript/README.md) guides.

- Effort selects a compiled mode. A new scanner is needed to change it.
- Formats default to EAN13; explicit selections and retail/common1D/common/1D/2D/all presets are supported.
  Python can override formats for a single call. JavaScript permits per-call subsets
  of its creation formats, so scanning stays synchronous without loading engines.
- Optional EAN/UPC supplement policy is fixed at creation: `Ignore` (default),
  `Read` or `Require`. It is independent of effort; nonretail formats are unaffected.
- `debug` adds diagnostic search evidence. Decoded text, geometry, support and
  supported semantic metadata are always returned.
- Python directly accepts Pillow images, NumPy arrays and tensors, with explicit
  `layout` and `value_range` overrides for ambiguous arrays/tensors. GPU tensors
  detach and transfer to CPU without changing the input or autograd graph.
- Python float arrays/tensors default to [0,1] regardless of their contents; byte-unit floats require `value_range="0_255"`. Optional `color_order="BGR"` supports OpenCV without affecting default RGB calls or adding a dependency.
- Raw Python buffers use `PixelImage(data, width=..., height=..., channels=..., stride=...)`.
  JS accepts ImageData or an object with data, dimensions and channels. Both default
  stride to packed rows; Python defaults channels to grayscale.
- Native library paths, browser WASM directories and advanced WASM loading remain
  configurable. Neither binding decodes image files or manages cameras.

## Ownership and evidence

Python snapshots input bytes before native scanning. JS callers keep input pixels
stable during synchronous scanning. Reusable scanners release resources through
Python context managers / `close()` and JavaScript `dispose()` in `finally`.

Candidate evidence and undecoded coverage remain separate from decoded results.
Search coverage does not promise exhaustive decoding. Support is an uncalibrated
ranking heuristic with reader-specific meaning; it is not comparable confidence
across formats or effort modes. Validated decodes can still be wrong. Recovered
candidate indices are local to their crop, not tracking identifiers. `unfinished`
combines decoding, localization, candidate-selection and parsing limits without
invalidating returned reads. False is not a guarantee of exhaustive scanning. Effort limits remain explicit inside
the implementation; arbitrary budgets and timeouts are not public scan options.

Python's `as_dict()` exports JSON-compatible public results without diagnostics, with original payload bytes represented as integer lists.

Python's `to_raw_dict()` exports independent native schema-2 JSON data. JS exposes
immutable raw evidence through `result.debug`; `structuredClone` makes a mutable
copy. Raw engine schemas are distinct from the compact public result.

Matrix decoders expose original decoded payload bytes through Python
`payload_bytes` and JavaScript `payloadBytes` when available; there is no text
re-encoding fallback. JS `scanner.formats` exposes its immutable configuration.
Diagnostics provide a stable collection of `UndecodedRegion` geometry records,
separate from decoded `Barcode` objects, with explicit absence
for unavailable proposals/search windows. Engine-specific evidence remains raw.

## Native integration and validation

Native consumers require matching ABI-4 libraries. C provides supplement flags;
Rust provides an explicit supplement-policy method. Library initialization checks
the required ABI and reports mismatches.

Python validates the 32-megapixel limit before image conversion or pixel copying.
Native errors retain numeric status codes and provide descriptive messages.

## Candidate continuation

`finish_candidates=True` (Python), `finish_candidates: true` (Rust), and
`finishCandidates: true` (JavaScript)
are per-scan options, disabled by default. They remove shared frame retry and
association budgets in the primary EAN13/UPC-A reader, including source-detail
recovery. The selected formats must contain EAN13 or UPCA. C uses flag 16
(`BARCODE_FINISH_CANDIDATES`); C++ and Java expose corresponding scan options.
`barcode_capabilities()` bit 0 advertises native support; the region WASM ABI
advertises it through `regions_completion_supported()` and uses its own flag 32.

Per-candidate effort, intentional weak-candidate deferral, coverage reuse,
localization, sampling, result and ambiguity limits still apply. Additional
readers retain their budgets. There is no wall-clock deadline in the library.
This can increase latency, and `unfinished` remains truthful rather than being
forced false. Completion is not equivalent to finding every barcode in an image.

Duplicate observations of a linear barcode can be merged when aligned bands are
connected by source-image bars and spaces. Equal payloads alone are insufficient:
separate products and differing supplements remain separate. Exhausting the
bounded duplicate-evidence budget preserves unchecked observations.
