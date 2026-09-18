# Shared retail integration

The default Medium mode uses the validated shared profile reader when EAN8 or
UPCE is selected with the default `Ignore` supplement policy. It samples primary
EAN13 evidence once, adds short-family decoding, and runs bounded recovery.
EAN8-only and retail selection collect the same evidence and filter output formats
afterward. EAN13-only keeps its primary path. Other effort policies and opt-in
supplement policies retain their existing reader selection.

The imported variant is `faithful`; optional geometry extension and unrestricted
deformation recovery are excluded. New immutable recipes and source hashes are
recorded in [promotion provenance](../provenance/promotion-shared-retail-20260918.json).
Native shared scans normalize input to packed RGBA to preserve the browser's
interpolation order. Duplicate consolidation runs once and preserves separately
printed equal labels, including a one-pixel separating gap.

## Validation

On the 68-photo human-reviewed EAN8 development panel, the production path recovers
60 labels versus 42 for the previous retail reader. Its outputs match the selected
research wrapper. EAN8-only and retail selection agree; native and WASM agree on
values, support and geometry to five decimal places. EAN13-only outputs match the
previous reader on this panel. These are development examples selected through
review discrepancies, not an independent accuracy holdout or a geometry benchmark.

A fresh Chrome session used two warmups and five timed repetitions per image,
with alternating method order. Median paired per-image runtime changes were:

| Comparison                            |  Change |
| ------------------------------------- | ------: |
| EAN13-only versus previous EAN13-only |  +0.12% |
| Shared retail versus EAN13-only       |  +7.53% |
| Shared retail versus previous retail  | −23.25% |
| EAN8-only versus shared retail        |  −0.08% |

The shared-retail overhead was 10.98% at the 90th percentile and 11.55% at the
95th percentile of paired ratios. The 10% target holds at the median, not for
every image. Bounding-box quality, duplicate outputs, and remaining short-code
misses remain follow-up work.

All four selected modes were rebuilt and verified. Checks include the complete
quality gate, 29 JavaScript tests, native/WASM and C/C++/Python/Java checks,
240 supplement mode/policy cases, source-detail recovery, image adapters, installed
Python wheel and JavaScript Node/browser/Worker installations, production demo
workers, and 720 standalone Rust/native
parity cases. Standalone Rust tests also pass with and without default features.

Package and demo builds are local validation artifacts; integration does not
publish packages or deploy the demo.
