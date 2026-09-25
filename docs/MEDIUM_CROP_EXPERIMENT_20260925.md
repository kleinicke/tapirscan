# Exact recovery-crop resampling

The selected runtime includes the pixel-identical optimization from experiment
`82dfe65b063f93b672925890ce96b91b66543510`, integrated onto current main while
preserving private Turbo work. Medium, High and Very High share this path; Low
and pure 2D scanning do not enter it. No API, preset, search budget, duplicate
policy or returned geometry changes.

Bounded crops are enlarged exactly threefold. Integer numerators over nine and
two cached horizontal rows produce the same output bytes as scalar floating
interpolation. Odd denominator nine avoids rounding ties. Tests cover all byte
pairs, 65,536 two-dimensional palette combinations, padded channels/strides and
maximum crop dimensions.

Paired development evaluation against the prior published Medium:

| Selection/runtime                       | Mean before | Mean after |
| --------------------------------------- | ----------: | ---------: |
| Common1D Chrome, 5,114 frames           |    33.29 ms |   23.27 ms |
| Retail Chrome, 5,114 frames             |    31.96 ms |   20.33 ms |
| Common1D native, 5,114 frames           |    21.21 ms |   18.45 ms |
| Common Chrome, 282-frame 2D/mixed panel |    76.00 ms |   62.39 ms |
| 2D-only Chrome, same panel              |    57.73 ms |   57.66 ms |

Full outputs excluding timing matched on 5,114 Common1D/Retail and 5,995 All-format
comparisons in each runtime. High and Very High also matched on 354 cases per
runtime. The 282-frame Common/2D panel matched in both runtimes. These correlated
development examples are not unseen holdouts or universal timing guarantees.
Large original photographs often barely benefit. Native photo timing was sensitive
to engine setup order; sharing the input allocation removed the apparent regression.

Retained research records: `medium-speed-transfer-20260925`,
`medium-common2d-check-20260925`, and current-main integration
`medium-crop-main-integration-20260925` in the experiment workspace. Current-main
integration adds fresh public Rust/ABI/build checks and exact image comparisons.
Provenance uses new immutable crop-integer identities; historical records remain
unchanged. Publication remains a separate operation.
