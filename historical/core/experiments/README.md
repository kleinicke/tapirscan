# Pinned mode experiments

`guarded-quality.patch` adds the feature-gated invalid-checksum consensus
guard to a copy of the current core. It does not change the checked-in scanner
source, the stable `fast` alias, the stable `quality` alias, or their frozen
artifacts.

The patch's manifest pins every modified base file, the unchanged lock and
fixture inputs, the post-patch file hashes, the full expanded feature closure,
and expected guarded WASM SHA-256
`f0ab293f3658e41936d130773e18741d91c4aaafded01dd1ee35f5ce3cb7ae64`.

Use a new output directory:

```sh
python3 rust/barcode-core/experiments/build_guarded.py --out /private/tmp/guarded-quality-build
```

The helper checks the 10 GiB reserve, refuses an existing output path, verifies
the base hashes before copying only the declared crate files, applies the
patch, verifies target hashes, runs the guarded-quality all-target tests, then
builds the expanded feature closure and checks the immutable WASM hash. Add
`--prepare-only` to inspect a patched temporary source without compiling.


## Sampling inline

`sampling-inline.patch` changes only the compiler inlining annotations on the
original-image bilinear and grayscale samplers. The formulas and scanner policy
are unchanged. The selected artifact is built with the expanded Fast recipe:

```sh
python3 rust/barcode-core/experiments/build_guarded.py --recipe sampling-inline --out /private/tmp/sampling-inline-build
```

Its immutable SHA256 is
`f61cadd6f9cb84b0b47bb61803503f7e6e323cea679e5f5eec7ef3df0c148600`.
The helper retains its historical name and defaults to `guarded-quality`; both
recipes pin every copied input, their patch, target source and final WASM.
Separate source patches keep the earlier Fast/Quality builds reproducible while
letting the new demo tags use the validated experiments. No scanner source is
modified in place by either command.
