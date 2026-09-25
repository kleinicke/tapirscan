# Localized recovery and higher-mode ownership

Experiment: `high-recovery-20260925`, baseline `31b731a`.

Medium reuses bounded source profiles on localized non-retail proposals. It
reduces the ordinary global linear angle budget while retaining original ITF
coverage. Supplement-reading policies retain their original global effort.
Existing Medium crop recovery is unchanged.

High and Very High retain their original small crops, then allow up to three
512-pixel recovery crops with local proposal fitting. Their new non-retail
profile pass focuses on Code128; the existing full-frame readers handle other
formats. EAN13/UPC-A also receive bounded source-band recovery.

For higher modes, unresolved same-symbol ownership can sample up to 2,048
positions and trace source bars up to 1,024 pixels, in either direction, within
a 524,288 pixel-sampling budget. Existing contrast, continuity and distributed
bar-match requirements remain in force. Matching text or display-expanded
polygons alone never establish ownership. Separate same-value labels and white
separators remain covered by regression tests.

The 12 private development images contain 19 relevant linear instances after
excluding the explicitly deferred damaged jar. Original Medium found 8, High 13
and Very High 13. The candidate finds 14, 19 and 19 respectively; High and Very
High have zero duplicates on these images. These are development results, not
an untouched holdout or a claim of perfect detection on arbitrary images.

Detailed control comparisons, timing, artifact hashes and known residual errors
are retained in the sibling experiment workspace. One strongly degraded EAN13
control still acquires an extra duplicate in the higher modes; no relaxed
ownership rule was introduced to hide it. Publication is a separate action.
