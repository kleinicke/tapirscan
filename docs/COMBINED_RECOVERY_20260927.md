# Bounded recovery for combined format selections

September 2026 development experiment. Medium, High and Very High share bounded
recovery work across enabled formats. The initial readers keep their established
coverage; a successful read never ends the entire image scan.

## Policy

- Reuse grayscale and existing localized candidates. Sample legal DataMatrix
  grids and validate Aztec mode/full-symbol hypotheses directly against source
  pixels. Strong border/mode evidence is tried before weaker size guesses.
- Share matrix source-sampling and localized crop allowances across the selected
  readers. QR, DataMatrix and Aztec can retry unresolved small regions with
  original pixels, local contrast restoration and twofold interpolation.
- Decode selected Code128, Code39, ITF and Code93 jointly on a bounded number of
  unresolved linear proposals. Medium and High allow eight proposals; Very High
  allows sixteen.
- Keep the existing Retail and Low algorithms. This change does not turn a
  combined selection into repeated complete single-format scans.

| Additional shared allowance    |  Medium |      High | Very High |
| ------------------------------ | ------: | --------: | --------: |
| Matrix source samples          | 250,000 | 2,000,000 | 4,000,000 |
| Matrix source crop area        |  65,536 |   131,072 |   262,144 |
| Pending matrix candidates      |       8 |         8 |         8 |
| Linear proposals               |       8 |         8 |        16 |
| Retail-conflict source samples |       0 |         0 |    32,768 |

Sample counts and source areas are separate work units, not wall-clock promises.
The initial scanner passes retain their existing limits.

## Correctness controls

Row agreement alone could repeat a systematically wrong low-resolution Code39
value. New Code39 recovery therefore uses original signal, independent rows and
at least two source pixels per narrow element. Sharpened Code39 recovery was
rejected. Extra ITF recovery also requires independent row agreement.

Recovered bands are joined only with matching metadata and bounded source
continuity evidence. A new overlapping same-value claim with unresolved physical
identity remains pending; established reads and separate same-value symbols are
preserved. Deferred claims survive generic resolved-region cleanup. Matrix/Rune
work skipped by candidate, source-area or sample limits reports unfinished work.

The expanded Very High photo check exposed an unchecked ITF interpretation of an
already-correct EAN-13. Very High now defers such a new ITF/Code39 claim when three
of that claim's own source rows decode the established checksum-valid retail
value across the same complete width. Overlap alone is insufficient. Separate
claims remain available. This extra admission rule is isolated to Very High;
the final Low, Medium and High WASM bytes match their earlier measured assets.

## Evidence and limits

Paired installed-Chrome measurements on a 443-image development panel. Very High uses the final admission guard; Low/Medium/High retain identical measured bytes:

| Mode / Common |     Target hits |       Median ms |         Mean ms |          p95 ms |
| ------------- | --------------: | --------------: | --------------: | --------------: |
| medium        | 248 → 256 / 392 |   49.30 → 51.80 |   68.98 → 71.89 | 203.40 → 206.50 |
| high          | 252 → 267 / 392 | 148.20 → 152.20 | 165.97 → 170.08 | 336.90 → 343.80 |
| very-high     | 252 → 270 / 392 | 174.20 → 178.50 | 191.52 → 197.02 | 402.70 → 422.90 |

The initial twelve mode/preset combinations (Retail, Common1D, Common, All) retained every previously hit annotated target under the recorded scoring. Complete-label panel controls gained no new unmatched physical read. Known existing errors remain. Medium/Common1D mean runtime increased about 7.3%; the final Common increases were 4.2%, 2.5% and 2.9% for Medium, High and Very High.

Expanded source-set checks and exact identities are retained in experiment `combined-recovery-20260927`.

On all 5,395 variation images, Medium/Common improves annotated target hits from
1,771 to 1,921 out of 4,779 eligible targets. Mean scan time changes from 49.65 to
52.16 ms (+5.1%), median from 43.50 to 45.90 ms, and p95 from 86.30 to 91.60 ms.
Known duplicate reads decrease from 21 to 14. The two previously quarantined
UPC-A source groups and their derivatives remain excluded from quality scoring
and included in timing. These measurements are paired against the pre-experiment
canonical source, not a claim about the published package.

The 150 additional Common target matches comprise Code128 +60, DataMatrix +59,
QRCode +23 and ITF +8. These counts include derivatives of the same sources.

Medium/All on the same full view improves target hits from 2,880 to 3,122 out of
7,326 eligible targets. Mean scan time changes from 72.38 to 75.63 ms (+4.5%),
median from 64.80 to 67.60 ms, and p95 from 116.20 to 122.80 ms. Aztec adds 56
matches and Code93 adds 36 beyond the Common gains. Both full Medium views retain
every previously matched target, reduce known duplicates from 21 to 14, and add
no unmatched predictions in quality-eligible photos. Incomplete photo labels do
not establish that all existing predictions are correct.

All imagery is development evaluation data. Variation images share 80 source
groups; derivatives are not independent captures or unseen evaluation. Photo
labels cover selected targets and may omit other barcodes. Complete-label
synthetic controls support wrong/duplicate physical-instance checks. Existing
errors remain; a clean regression comparison is not a general correctness proof.

Chrome measurements use reusable scanner sessions and alternating paired engine
order on complete images. Scan timing includes scanner preprocessing and excludes
file decoding, fetching, WASM download and scanner creation. Absolute times depend
on device, browser, imagery and selected formats. Source and immutable binary
identities are recorded in the selected provenance manifests. Package and demo
publication is a separate operation.

Final Very High/All checks cover all 2,760 original/derivative images in eight
linear source families (Retail, Code128, Code39, ITF, Code93), with every format
enabled. Target hits improve from 1200 to 1535 / 3795; mean scan time
changes from 203.12 to 210.40 ms and median from 194.30 to 200.10 ms.
The final counterexample, complete Chrome controls and native atlas checks pass.
This targeted expanded view is not a full 5,395-image final Very High benchmark.
