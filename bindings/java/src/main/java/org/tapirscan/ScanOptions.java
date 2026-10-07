package org.tapirscan;

import java.util.Optional;
import java.util.Set;

/**
 * Optional overrides for one image.
 *
 * @param formats readers for this call; empty uses the scanner's formats
 */
public record ScanOptions(Optional<Set<Format>> formats) {
    public ScanOptions {
        formats = formats.map(Set::copyOf);
        if (formats.isPresent() && formats.get().isEmpty()) {
            throw new IllegalArgumentException("formats must not be empty");
        }
    }

    /** The scanner's formats. */
    public static ScanOptions defaults() {
        return new ScanOptions(Optional.empty());
    }

    public ScanOptions withFormats(Set<Format> value) {
        return new ScanOptions(Optional.of(value));
    }
}
