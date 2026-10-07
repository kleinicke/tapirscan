# Experimental Turbo scanners

JavaScript exposes the retained Turbo2, Turbo4, Turbo8 and Turbo16 recipes through
`experimentalTurbo: 2 | 4 | 8 | 16`. See the [JavaScript guide](../bindings/javascript/README.md#experimental-turbo-presets)
for usage, WASM imports, option restrictions and the experimental compatibility policy.
The presets target faster **1D scanning, including Retail**; they do not provide
corresponding 2D speedups. Numbers identify presets, not guaranteed multipliers.

The original Turbo policy remains public **Low** (TS-Low in the demo).
**TS-Low Classic** is the previous Low retained as a fixed demo comparison.
Turbo32 remains retired because it lost substantially more reads and did not reach
its speed target. See [Low and Low Classic](LOW_MODES.md).

Normal WASM package builds include four stable modes and the four experimental
presets from current source, each with an immutable binary identity and recorded
build policy. The scanner verifies the selected asset's preset identity.
Native language APIs are unchanged. The private builder below remains available
for native research and explicit development artifacts.

Build an explicit private artifact:

```sh
python3 scripts/build_turbo.py original --native --wasm
python3 scripts/build_turbo.py 16 --wasm
```

Outputs and their build environment are recorded under `build/private-turbo/`.
The ordinary builders are used internally, with native output directed to the
private directory so the ordinary `build/native` libraries remain unchanged.
Private WASM compilation uses development assets and does not change
release identities or publish a package or demo. Select the private file
explicitly; it still uses the Low adapter ABI. JavaScript selects numbered presets through `experimentalTurbo`; stable mode names
and format identifiers are unchanged.

## Formats and quality

The principal speedups apply to the seven Common1D formats: EAN-13, UPC-A, EAN-8,
UPC-E, Code128, Code39 and ITF, including subsets and arbitrary rotation.
`Common` additionally enables QR Code and Data Matrix. `All` enables all 16
supported symbologies, including PDF417, Aztec and MaxiCode; these remain supported
by the private artifacts. Format support does not imply identical detection
quality or the Common1D speed ratio on every format.

The tiers reduce the localization grid, sampling rows and recovery work. Their
numbers are experimental Common1D speed targets, not guarantees on a particular
image or multipliers for 2D decoding. Bounded axis confirmation preserves the two nearby equal EAN8 labels in the
regression control, including at Turbo16. Aggressive presets still leave
unexamined candidates and can miss other clean nearby symbols.
Small modules, blur, damage, distortion,
difficult lighting and crowded scenes can lose reads relative to Medium. There
is no reference-decoder or neural fallback. The source image is never assumed to
contain only one symbol; `multiple: false` ranks after scanning all candidates.

Every fast linear Turbo result reports `unfinished: true`. Source-coordinate
geometry, undecoded candidates and support ranking remain available. Add-on
policies other than Ignore and extended linear budgets retain the ordinary
pipeline. The public API and release assets are separate from demo labels.

## Bounded linear confirmation

Turbo8 and Turbo16 can confirm weak whole-image axis observations for Retail,
Code128 and ITF using their existing finite refinement budgets. Code39 keeps its
original axis policy. The two nearby equal EAN8 labels in the regression control
remain separate physical results, including at Turbo16.

When the ordinary scan finds no reads, Turbo2, Turbo4 and Turbo8 can examine one
previously omitted merged stripe proposal. Turbo2 requires four distinct rows
from the initial sampling grid for that proposal. Numbered presets can also retry
one weak Retail observation with bounded contrast profiles, requiring the same
payload and position on three source rows. Optional EAN13 and UPC-A recovery must
pass source-template agreement and contradiction checks. These paths retain the
physical continuity rules and never assume that an image contains only one label.

When a Retail scan reads nothing, Turbo2 also retries its strongest proposal with Low's
contrast and high-pass profiles, under the same source-agreement and contradiction
checks. Unlike Low it skips camera-shake deghosting and the second proposal, which
keeps empty frames cheap. On the development timing subsets this added 15 of 1,170
labeled Retail reads (833 to 848) without wrong reads, for +0.18 ms per Retail
image and +0.31 ms per empty frame. Low's segment-voting localization was rejected
for Turbo: it made Turbo2 about as slow as Low. See `turbo-review-20261006`.

## Matrix fast path

All five retained Turbo recipes use the same conservative 2D policy. After
shared grayscale conversion, a constant frame needs no matrix search. Otherwise,
when generous padding around all non-background pixels occupies at most 25% of
the frame and spans at most 512 pixels per side, scan that complete
source-resolution crop. Busy foreground spans reject the crop early. Do not stop after finding
a first symbol. Failed crops and crops with unresolved regions run the original
full-source search. Translate successful crop geometry back to source pixels and
report unfinished work. Busy images keep the full-source readers.

This preserves small modules and improves sparse-frame performance, but crop
context changes can affect detection. On 2,821 development images it retained
all 683 baseline 2D reads without added or lost payloads. On 95 full-HD synthetic controls
it preserved all 89 baseline reads, including rotated and unequal-sized copies;
QR, Data Matrix, PDF417 and Aztec passed every control. MaxiCode retained its
existing misses. Do not describe this as exhaustive decoding or uniform speedup
on photographs. A 768-pixel resolution cap was rejected after substantial losses.

## Evidence and follow-up

Reproducible sources, native/WASM identities, paired quality and performance
measurements, rejected shortcuts and Medium transfer ideas live in the sibling
experiment workspace under `common1d-turbo-tiers-20260925`,
`common1d-turbo16-32-20260925`, `turbo-2d-integration-20260925`,
`turbo-quality-20261002` and `turbo-review-20261006`.
These are development datasets, not unseen holdouts or phone performance claims.

## Beyond Common

Private builds also decode Codabar, Code93, DataBar and DataBar Expanded on
oriented Turbo proposals. Codabar requires at least three distinct source-row
confirmations. The existing pixel-continuity ownership checks now include these
formats, preserving adjacent equal-payload symbols while consolidating repeated
observations of the same physical barcode. Complementary full-image readers
remain available, including their stacked DataBar handling.

The bounded sparse-frame preparation is shared between additional linear and
matrix readers. Each group independently retries the original source if its crop
fails or retains unresolved regions; success in one group never suppresses the
other group's recovery. Common1D-only decoding keeps its existing policy.

The original policy is public Low; JavaScript selects numbered tiers with the
experimental option rather than adding stable effort-mode names. The
`turbo-all-formats-20260925` experiment in the sibling workspace records per-format
quality, strict and presentation-normalized payload metrics, duplicate controls,
paired Chrome timing and rejected alternatives. Tier numbers are still targets
for Common1D work, not All-format speed guarantees.

## QR improvements from September 27

Fresh builds of all five retained recipes inherit the maintained Low QR path,
including exact alignment-coordinate caching, version-header rejection and the
bounded large-image recovery added in `qr-speed-quality-20260927`. The numbered
recipes retain their distinct linear policies; their tier numbers are not QR
speed multipliers. See [QR effort transfer](../core/README.md).

The historical assets in `demo/src/lib/turbo.json` remain immutable comparison
pins. Rebuild with `scripts/build_turbo.py` and explicitly select the resulting
private artifact to use current source. This does not publish or repoint the demo.
