# Experimental barcode outline fallback

The existing physical-bar tracing and ownership decisions run first. If they
cannot establish an extent, remaining source-sampling budget may recover a
larger display quadrilateral through barcode-profile agreement. This geometry
is not used as proof for suppressing another barcode.

The fallback samples 256 points across the decoded band. It removes local
brightness trends and requires agreement in six of eight distributed sections,
with a bounded local phase adjustment. It advances in proportion to symbol
width, with a 128-step limit per direction. Half-pixel continuity probes remember
light breaks across rows, including thin separators. Both phases share the
existing 262,144 source-sample limit. Endpoints from exhausted work are rejected.
Corner winding is normalized when checking that the result enlarges the band.

A quadrilateral remains an approximation: curved or uneven ends and long guard
bars can extend outside the consensus outline. This is not a pixel segmentation
or a guarantee of complete boundaries. Values, search scheduling and duplicate
ownership rules are unchanged.

The initial six-track relaxation was rejected: it still failed or produced skewed
extents. The first profile prototype mishandled reversed corner winding; a later
prototype spent budget before subsequent ownership tracing. Both were corrected
before the final replay, with the failed outcomes retained in the experiment.

Validation: 9,984 old/new scored comparisons across development controls and
variations; no payload/count changes and no per-image losses at IoU 0.01, 0.3 or
0.5. Medium has the full 3,837-image cohort for Retail and Common1D; the other
three modes use 385 selected controls each. The 20 supplied/rescaled-photo cases
have 160 all-mode comparisons. Medium native/WASM parity agrees on 398 cases.
All 1,253 unit, 8 API and 2 doc tests and strict all-target Clippy passed.

Evidence is retained in the sibling experiment workspace under
`retained/footprint-consensus-20260924/`. These are development evaluations,
including reused source photos and generated derivatives, not unseen holdouts.
Browser timing is recorded separately in that report. This experiment does not
publish a package or deploy the demo.

Paired Chrome measurements across 53 images were effectively unchanged: Retail
mean 41.52 to 41.35 ms and Common1D 49.04 to 49.09 ms. A focused 15-repeat
measurement of the original 3024 by 4032 photo had median paired increases of
1.6 ms for Retail and 1.7 ms for Common1D. This improves displayed coverage;
it is not a decoding speedup. Timing excludes image loading and compilation.
