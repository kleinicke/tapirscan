# Very High subpixel EAN-13 recovery

Very High is the intended effort tier for resolving the subpixel decoding
challenge as Tapirscan develops. This first integration adds an original-pixel
recovery path; it does not establish a guaranteed camera resolution or solve
subpixel localization. Low, Medium and High keep their previous behavior.

## Selected implementation

After ordinary Very High retries, unresolved affine candidates with at least
four observed intensity runs can use pixel-area
reconstruction and direct legal EAN digit-sequence fitting. Gray pixels are
weighted mixtures of several modules; rotation gives different rows different
sampling phases. This preserves information lost by independent scanline
thresholding or enlargement of an already sampled bitmap.

The bank tries sharp reconstruction and sequence fits with optical blur sigma
0, 0.15, 0.3 and 0.5 source pixels, at intensity powers 1, 0.45 and 2.2. Conflicting
accepted values reject the bank. The strongest visual digit sequence must pass
its checksum; the checksum never repairs it or promotes a weaker sequence.
Two disjoint bands must independently decode the same value using the selected
model. Those observations pass through ordinary source-continuity association,
invalid-checksum/conflict vetoes and frame reconciliation. Their support is two
independent bands, never the number of fitted models or sampled pixels.

The kernel accepts rectangular candidates 28.5–114 pixels wide (0.3–1.2
pixels/module), 12–160 pixels high, with an expanded source region at most
16,384 square pixels. Perspective and shear outside a 0.05-pixel tolerance are
rejected. Each axis reserves 17 model slots: 15 hypotheses and two support fits.
At most 64 fits run per core frame, even with extended effort. Existing frame,
per-candidate and retry-mask limits also apply. Already-decoded candidates are
not modified; exhausted work remains pending only for eligible bar-like candidates.
Blank images do not acquire unfinished work from this fallback. No fallback scanner or learned
model is introduced.

## Evidence

Baseline: `33375b022825341a08d405fd3e156bbefd22508d`.
Research origin: `ef995bf83ee3736a840f554e75fa57156f1f9c6b`.
Reproduction and full observations are retained in experiment
`subpixel-very-high-20260926` in the companion experiment workspace.

The integrated supplied-region pipeline read 4,728/5,376 sharp synthetic cases
without wrong reads. Per-pitch results were 309/768 at 0.3, 615/768 at 0.4,
735/768 at 0.5, 767/768 at 0.6, 768/768 at 0.69 and 0.8, and 766/768 at 1.0.
These used 32 payloads, 12 orientations, two pixel phases and 40-pixel-high bars.
The production support rules are stricter than the original research harness.

The paired sharp result is **1,021 → 4,728** exact reads, 3,707 gains, zero
losses and zero wrong reads. On 4,032 mildly blurred/noisy cases the result is
**1,238 → 2,849**, 1,611 gains and zero losses. Seven wrong reads already present
in the baseline remain identical. On 12,840 synthetic negative controls, both
versions emit the same three false reads; the integration adds none. These
controls are correlated synthetic cases, not a real-world false-positive bound.

All **920 existing-image cases** return identical public outputs: the frozen
500 EAN-13 scenes, all 80 all-format source images, and their 340 EAN-13
variations. Thus this integration shows no new recall gain on those full images.
Source annotations, derivatives and benchmark source groups remain unchanged;
these are overlapping development selections, not independent held-out captures.

Native paired timings on an Apple M1 Pro use 50 evenly selected cases from each
comparison, five repetitions, warmup and alternating execution order. On core500,
median time is 125.00 → 121.97 ms and mean time 146.47 → 140.35 ms.
On the additional selection, EAN-13 variation medians are 71.76 → 75.54 ms
(+5.3%); means are 69.86 → 75.25 ms (+7.7%). All-format original medians
are 308.45 → 303.02 ms. Mixed differences across repetitions and groups
should not be interpreted as a general speedup or a fixed overhead. The supplied-region
synthetic stress median is roughly 12 ms after the complete hypothesis bank,
which is why this path belongs to Very High rather than Medium.

All four core-mode tests and strict Clippy checks pass, as do four-mode native
builds and ABI tests. All 1,318 public Rust tests and 22 JavaScript tests pass,
as do Rust/native parity and the selected binding/continuation checks. Native
and core-WASM text sets match on 672 supplied-region cases. Installed Node,
Chrome (default and relocated assets) and worker checks pass using the reviewed
fixture described below. Exact commands and observations are retained in the
experiment outcome. The diagnostic example `core/examples/subpixel_regions.rs`
accepts only pixels and quadrilaterals; labels stay in the external evaluator.

### Existing package-fixture discrepancy

The unmodified installed-consumer fixture expects `unfinished: false` for a
blank Code128-only image. The baseline returns `true` in all four native modes
under both budget settings, with no barcode values. This predates subpixel
recovery and is outside its EAN-13 path. Installation comparisons use a separately
recorded baseline-reviewed manifest changing only that one expectation to `true`;
payload and geometry assertions, production code and the original source fixture
remain unchanged. This is a documented fixture/contract discrepancy, not a claim
that the unmodified installed-consumer suite passes.

## Continuing development

The objective is for Very High to solve subpixel codes reliably on real images,
with 0.6 pixels per narrow EAN bar as an initial synthetic reference point.
Precise active-symbol boundaries remain the main dependency. Development should
measure and improve boundary phase, width, orientation and perspective before
expanding the acceptance envelope. Camera response, blur and geometry interact;
the original guard-only brightness estimator did not generalize and is not
included. Neither the isolated 0.035-pixel noiseless success nor the noisy
0.05-pixel failure is an operating specification; this implementation excludes
that unstable range.

Next steps are bounded geometry refinement, calibrated intensity handling,
real-image negative controls, and paired phone/browser measurements. Localization
must be evaluated separately and then end to end. QR/Data Matrix need a separate
2D cell model and evidence-aware error correction; the repeated-height advantage
of a linear barcode does not transfer. This roadmap describes the intended role
of Very High, not functionality already achieved by every camera image.
