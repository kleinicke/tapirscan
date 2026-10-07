package org.tapirscan;

import java.util.List;
import java.util.Optional;

/**
 * Owned scan output; it remains valid after its scanner is closed.
 *
 * @param undecoded localized but unread regions
 * @param unfinished the engine reported a work limit; false does not guarantee exhaustive scanning
 * @param diagnostics unstable engine diagnostics JSON from inspection
 */
public record InspectionResult(
        List<Barcode> barcodes,
        List<UndecodedRegion> undecoded,
        int width,
        int height,
        Mode mode,
        double elapsedMs,
        boolean unfinished,
        String diagnostics) {
    public InspectionResult {
        barcodes = List.copyOf(barcodes);
        undecoded = List.copyOf(undecoded);
    }

    /** Highest support, keeping the first read on ties; see {@link Tapirscan#best}. */
    public Optional<Barcode> best() {
        return Tapirscan.best(barcodes);
    }

    /** Decoded text of every barcode, in scanner order. */
    public List<String> values() {
        return barcodes.stream().map(Barcode::text).toList();
    }
}
