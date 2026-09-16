# How Tapirscan works

Tapirscan's main EAN-13 path combines oriented localization with a supplied-region
scanner. It uses classical image processing and decoding; neither neural weights
nor another barcode library are required at runtime.

## From pixels to results

1. **Find candidate regions.** Stripe structure proposes barcode-like areas and
   orientations. Selected proposals receive geometric refinement.
2. **Sample original pixels.** Oriented profiles follow each candidate's geometry;
   a full-frame search is also retained.
3. **Attempt all primary candidates.** Cheap attempts precede bounded retries.
   Source evidence decides where the new modes permit additional retry work.
4. **Recover small details.** Medium, High and Very high inspect up to two source
   texture seeds, enlarge selected 256-pixel crops threefold, and scan them with
   a separately pinned Low decoder.
5. **Reconcile and return.** Decoded values have source-coordinate polygons and
   support scores. Optional evidence retains undecoded regions, work limits,
   primary candidates, and recovery frames with explicit crop transforms.

The effort modes change both compiled implementations and host budgets. They are
not merely aliases for one function with different timeouts. Higher effort does
not guarantee a strict superset of a lower mode's reads. Bounded recovery remains
marked unfinished, even when it successfully decodes a symbol.

## One algorithm family, two execution environments

JavaScript loads Rust/WASM and orchestrates the pipeline synchronously after
initialization. Browser recovery uses Canvas interpolation; Node uses a software
bilinear implementation. The native Rust facade ports the same orchestration
and uses the same separately compiled recovery core. C, C++, Python and Java
all call that native ABI.

Grayscale, RGB and RGBA inputs support explicit strides; alpha is ignored.
Positions refer to the pixels supplied by the caller, not an earlier image before
resizing. Crop-local candidate indices are never presented as primary indices.
See [the native contract](NATIVE_BINDINGS.md) and [Python input rules](API_DESIGN.md).

## Additional formats

`multiformat/` contains independent, opt-in experimental readers. JavaScript loads
that WASM only when requested formats need it. EAN-13 and UPC-A retain the selected
primary effort mode. Common1D uses effort 0/1/2/2 and QR Code uses 0/1/2/3
for Low/Medium/High/Very High; other matrix readers use effort 1.
When mixed with EAN13/UPCA, confirmed Common1D coverage can defer deep EAN
retries only for wholly contained proposals. Initial discovery and full-frame
search remain enabled. Source-detail recovery skips seeds inside that coverage.

These readers have different maturity and incomplete work-limit propagation in
some paths. Consult [format coverage](FORMATS.md); do not infer QR reliability
from an EAN-13 result.

## Repository map

| Directory                                 | Purpose                                              |
| ----------------------------------------- | ---------------------------------------------------- |
| `core/`                                   | Frozen base sources and exact experiment patches     |
| `provenance/`                             | Selected mode recipes and source/binary hashes       |
| `bindings/javascript/`                    | Browser/Node API and WASM orchestration              |
| `bindings/rust/`                          | Safe native facade and source-detail recovery port   |
| `bindings/c/`, `cpp/`, `python/`, `java/` | Native language interfaces                           |
| `multiformat/`                            | Pinned experimental additional readers               |
| `demo/`                                   | Camera/photo app with independent comparison workers |
| `scripts/`                                | Reproduction, packaging and regression checks        |

Builds apply recipes in generated directories. Two documented native adaptations
expose an existing helper and rename the recovery package to prevent Cargo feature
unification. Neither rewrites the frozen algorithm inputs.
[The current promotion](PROMOTION_DETAIL_20260914.md) records exact selections and
integration evidence. Historical promotion records remain available for audit.

The demo's ZXing and ZBar workers are comparison tools. They never supply fallback
results to Tapirscan, and they are not dependencies of the distributed library.
