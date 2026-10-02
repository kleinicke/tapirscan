package org.tapirscan;

import java.util.List;
import java.util.Optional;
import java.util.OptionalInt;

/**
 * Owned scan output; it remains valid after its scanner is closed.
 *
 * @param undecoded localized but unread regions
 * @param unfinished the engine reported a work limit; false does not guarantee exhaustive scanning
 * @param debug engine diagnostics JSON, present only when {@link ScanOptions#debug()} was set
 * @param bestIndex position of {@link #best()}
 */
public record ScanResult(
        List<Barcode> barcodes,
        List<UndecodedRegion> undecoded,
        int width,
        int height,
        Mode mode,
        double elapsedMs,
        boolean unfinished,
        Optional<String> debug,
        OptionalInt bestIndex) {
    public ScanResult {
        barcodes = List.copyOf(barcodes);
        undecoded = List.copyOf(undecoded);
    }

    /** Highest support, keeping the first read on ties. */
    public Optional<Barcode> best() {
        return bestIndex.isPresent() ? Optional.of(barcodes.get(bestIndex.getAsInt())) : Optional.empty();
    }

    /** Decoded text of every barcode, in scanner order. */
    public List<String> values() {
        return barcodes.stream().map(Barcode::text).toList();
    }
}
