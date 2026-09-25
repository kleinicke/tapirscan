# Private Turbo scanners

The maintained source includes the original Turbo scanner (shown as **TS-Low** in
the demo) and optional Turbo2, Turbo4, Turbo8 and Turbo16 experiments. These are
private build recipes, not public API modes. The ordinary Low, Medium, High and
Very High builds retain their existing mode selections. Turbo32 was retired:
it lost substantially more reads and did not reach its speed target.

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
explicitly; it still uses the Low adapter ABI. No Turbo names are accepted by
public constructors, format enums or mode registries.

## Formats and quality

The principal speedups apply to the seven Common1D formats: EAN-13, UPC-A, EAN-8,
UPC-E, Code128, Code39 and ITF, including subsets and arbitrary rotation.
`Common` additionally enables QR Code and Data Matrix. `All` enables all 16
supported symbologies, including PDF417, Aztec and MaxiCode; these remain supported
by the private artifacts. Format support does not imply identical detection
quality or the Common1D speed ratio on every format.

The tiers reduce the localization grid, sampling rows and recovery work. Their
numbers are experimental Common1D speed targets, not guarantees on a particular
image or multipliers for 2D decoding. Small modules, blur, damage, distortion,
difficult lighting and crowded scenes can lose reads relative to Medium. There
is no reference-decoder or neural fallback. The source image is never assumed to
contain only one symbol; `multiple: false` ranks after scanning all candidates.

Every private fast linear result reports `unfinished: true`. Source-coordinate
geometry, undecoded candidates and support ranking remain available. Add-on
policies other than Ignore and extended linear budgets retain the ordinary
pipeline. The public API and release assets are separate from demo labels.

## Matrix fast path

All five retained private recipes use the same conservative 2D policy. After
shared grayscale conversion, a constant frame needs no matrix search. Otherwise,
when generous padding around all non-background pixels occupies at most 75% of
the frame, scan that complete source-resolution crop. Do not stop after finding
a first symbol. Failed crops and crops with unresolved regions run the original
full-source search. Translate successful crop geometry back to source pixels and
report unfinished work. Busy images keep the full-source readers.

This preserves small modules and improves sparse-frame performance, but crop
context changes can affect detection. On 2,821 development images it retained
all 683 baseline 2D reads and added five QR reads. On 95 full-HD synthetic controls
it preserved all 89 baseline reads, including rotated and unequal-sized copies;
QR, Data Matrix, PDF417 and Aztec passed every control. MaxiCode retained its
existing misses. Do not describe this as exhaustive decoding or uniform speedup
on photographs. A 768-pixel resolution cap was rejected after substantial losses.

## Evidence and follow-up

Reproducible sources, native/WASM identities, paired quality and performance
measurements, rejected shortcuts and Medium transfer ideas live in the sibling
experiment workspace under `common1d-turbo-tiers-20260925`,
`common1d-turbo16-32-20260925` and `turbo-2d-integration-20260925`.
These are development datasets, not unseen holdouts or phone performance claims.
