# Public API design

This is the 1.2.0 API revision. See [migration](API_MIGRATION.md).
Python, JavaScript and Rust expose one scan operation returning `ScanResult`.
The public Rust `Scanner` owns the pipeline in every binding. JavaScript uses a
thin WASM adapter; the C ABI connects Python, C++, and Java to the same API.
One-shot calls clean up automatically; reusable scanners amortize initialization.
Rust uses `scan(image)` for defaults and `scan_with_options(image, options)` for
overrides; Python uses keyword arguments and JavaScript an options object. All return independent results, including empty results.

## Results

- `barcodes`: accepted decoded physical instances, including separate copies of
  the same payload. Text, format, source-image polygon and payload metadata remain
  available without debugging.
- `undecoded`: reported localized proposals without an accepted decode. These
  may be false candidates, failed attempts or deferred work; they are not a list
  of proven real barcodes. Entries may overlap. Empty does not prove coverage.
- `unfinished`: the engine reported a work limit or deferral. Reads remain usable.
  False does not promise every visible barcode was found.
- Image dimensions, selected mode and scan timing. Rust uses `Duration`;
  Python and JavaScript expose milliseconds.
- `debug`: optional engine-specific evidence. It does not control whether public
  decoded or undecoded geometry is returned. Raw schemas are not stable API.

`values` is a convenience projection of decoded text. `best` selects the largest reader-specific support,
keeping first-read ties. It is not a most-reliable selection across formats or
efforts and does not change scan work. Applications should select by the format,
payload or position they need. Support remains uncalibrated evidence.

Polygons use source-image pixels, origin top-left, x rightward and y downward.
`rect` encloses the polygon with floor(minimum) and ceil(maximum) pixel bounds in
all three bindings. Rust returns `[left, top, width, height]`; Python/JS use named
fields. GS1 and reader-initialization metadata preserve unavailable versus false.
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
The adapters translate this intent to existing engine controls; ABI 4 and pinned
recipes are unchanged. There is no retry-until-finished loop.

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
Use a worker for browser responsiveness. Results survive scanner disposal.

The native ABI remains version 4. C, C++ and Java retain their existing ABI-facing
interfaces. This revision changes the Python, JavaScript and standalone Rust
application APIs; publication and release versioning are separate steps.
