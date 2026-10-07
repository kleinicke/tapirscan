# Public API design

This is the 1.3.0 API revision. See [migration](API_MIGRATION.md).
`scan` returns a lightweight `ScanResult`: decoded strings through `values`,
and their locations through `barcodes`. Each barcode retains format, geometry,
support and optional payload metadata. `best` is an optional convenience.
There is no public `debug` option. Python and Rust also allow direct iteration.

Inspection is explicit and returns `InspectionResult`:

- `barcodes`: the same decoded instances as scanning with the same options.
- `undecoded`: localized proposals without accepted decodes. They can be false
  candidates, failed attempts or deferred work, and may overlap.
- `unfinished`: the engine reported a work limit. False does not promise exhaustive coverage.
- Image dimensions, selected mode and scan timing.
- `diagnostics`: unstable engine-specific evidence for advanced consumers.

C uses owned result handles for both operations. Count/barcode/copy accessors work
for either; summary and unread-region accessors require inspection. JSON is lazy:
ordinary results serialize to a barcode array, inspection to a schema-2 report.

`values` is a convenience projection of decoded text. `best(barcodes)` selects the largest reader-specific support
from any barcode list, including `result.barcodes`, keeping first-read ties;
`result.best` applies the same rule to either result type. It is not a most-reliable selection across formats or
efforts and does not change scan work. Applications should select by the format,
payload or position they need. Support remains uncalibrated evidence.

Polygons use source-image pixels, origin top-left, x rightward and y downward.
`rect` encloses the polygon with floor(minimum) and ceil(maximum) pixel bounds.
Rust and C++ return `[left, top, width, height]`; Python, JS and Java use named
fields; C applications compute bounds from the polygon. GS1 and reader-initialization metadata preserve unavailable versus false.
Payload bytes are original decoded bytes when available, never text re-encoding.
Supplements remain separate from the main text and polygon.

## Candidates and budgets

Localization proposes barcode-like regions. Initial discovery evaluates selected
candidates; additional retries depend on the effort policy and work budgets.
Source-detail recovery can discover additional candidates.

`extended_budget=False` is the default (`extendedBudget: false` in JavaScript,
`extended_budget: false` in Rust). Set it to true to allow additional reader work.
It is valid for every format selection, including selections changed per call.
The public contract does not prescribe candidate counts, iteration limits,
shared versus per-candidate budgets, or which internal search stages expand.
Those details may evolve without changing this API. Effort mode remains separate.

Currently, true removes the primary EAN-13/UPC-A shared retry and association caps.
Other readers currently keep their existing budgets; accepting the flag does not
claim that every reader already performs extra work. Future readers can extend
appropriate budgets under the same flag. Per-candidate effort, intentional
deferral, localization, sampling, result and ambiguity limits still apply.
It is not unlimited search or a deadline, and `unfinished` may remain true.
The adapters translate this intent to the engine’s work controls. There is no retry-until-finished loop.

## Configuration and ownership

Defaults are Medium effort, retail formats and ignored supplements. Effort and
supplement policy are fixed at creation. Per-call format selection does not
mutate configuration. JavaScript selections must be subsets of loaded formats;
native bindings can use any supported format without asynchronous loading.

Inputs are decoded pixels, not filenames, URLs or encoded images. Python accepts
Pillow/NumPy/tensors plus `PixelImage`; Rust accepts borrowed image buffers; JS
accepts `ImageData` or explicit byte buffers. Input validation and errors remain
binding-native. No detection is a successful empty result, never an error.

Python snapshots input and releases scanners with a context manager or `close`.
Rust borrows pixels synchronously and uses RAII. JavaScript initializes
asynchronously, scans synchronously and releases WASM sessions with `dispose`.
Use a worker for browser responsiveness: `tapirscan/browser` runs the same
scanner in a bundled worker, accepts browser image sources (files, image, video
and canvas elements, bitmaps), constructs synchronously and returns promises.
Results survive scanner disposal.

C exposes the same model through explicit structs and copy functions; C++ and
Java wrap it with the names used here and return owned results. See
[native bindings](NATIVE_BINDINGS.md).
