# Experimental Turbo presets

JavaScript offers four experimental presets for faster 1D scanning:
`experimentalTurbo: 2 | 4 | 8 | 16`. See the
[JavaScript guide](../bindings/javascript/README.md#experimental-turbo-presets)
for usage, WASM imports, option restrictions and the compatibility policy.
Other bindings do not offer the presets.

## What they speed up

The presets do less localization, sampling and recovery work on linear
barcodes: EAN-13, UPC-A, EAN-8, UPC-E, Code 128, Code 39 and ITF, at any
rotation. Higher numbers do less work. The numbers name presets; they are not
speed multipliers.

Other formats remain readable when selected, but the presets do not make them
proportionally faster. Mixed-format scans still pay for the enabled 2D readers.
On sparse frames, matrix readers first scan a padded crop around the foreground
and fall back to the full image when the crop yields nothing or leaves
unresolved regions.

## What they trade away

Small modules, blur, damage, distortion, difficult lighting and crowded scenes
can lose reads compared with Medium. Higher presets can also miss clean symbols
placed close together. Results keep source-image geometry and multiple
symbols, and equal labels at different positions stay separate results.

Presets require `eanAddOnPolicy: "Ignore"` and reject `extendedBudget: true`.
Use an effort mode when you need supplements or extended budgets.

Start with preset 2 and check detection on your own images before choosing a
higher one.

## Building preset artifacts

Package builds include all four presets. To build a single preset explicitly:

```sh
python3 scripts/build_turbo.py 16 --wasm
```

Outputs go to `build/private-turbo/` and do not change release artifacts.
