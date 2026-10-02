package org.tapirscan;

import java.util.Optional;
import java.util.Set;

/**
 * Optional overrides for one image.
 *
 * @param formats readers for this call; empty uses the scanner's formats
 * @param debug include unstable engine diagnostics as JSON in {@link ScanResult#debug()}
 * @param extendedBudget allow reader-specific extra work; not a deadline or exhaustive search
 */
public record ScanOptions(Optional<Set<Format>> formats, boolean debug, boolean extendedBudget) {
    public ScanOptions {
        formats = formats.map(Set::copyOf);
        if (formats.isPresent() && formats.get().isEmpty()) {
            throw new IllegalArgumentException("formats must not be empty");
        }
    }

    /** The scanner's formats, no diagnostics and the ordinary work budget. */
    public static ScanOptions defaults() {
        return new ScanOptions(Optional.empty(), false, false);
    }

    public ScanOptions withFormats(Set<Format> value) {
        return new ScanOptions(Optional.of(value), debug, extendedBudget);
    }

    public ScanOptions withDebug(boolean value) {
        return new ScanOptions(formats, value, extendedBudget);
    }

    public ScanOptions withExtendedBudget(boolean value) {
        return new ScanOptions(formats, debug, value);
    }
}
