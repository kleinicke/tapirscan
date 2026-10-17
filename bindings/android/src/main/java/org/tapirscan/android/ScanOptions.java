package org.tapirscan.android;

import java.util.Objects;
import java.util.Optional;
import java.util.Set;

/** Optional overrides for one image. */
public final class ScanOptions {
    private final Optional<Set<Format>> formats;

    /** @param formats readers for this call; empty uses the scanner's formats */
    public ScanOptions(Optional<Set<Format>> formats) {
        this.formats = formats.map(ScannerOptions::copy);
    }

    /** The scanner's formats. */
    public static ScanOptions defaults() {
        return new ScanOptions(Optional.empty());
    }

    public Optional<Set<Format>> formats() {
        return formats;
    }

    public ScanOptions withFormats(Set<Format> value) {
        return new ScanOptions(Optional.of(value));
    }

    @Override
    public boolean equals(Object other) {
        return other instanceof ScanOptions && formats.equals(((ScanOptions) other).formats);
    }

    @Override
    public int hashCode() {
        return Objects.hash(formats);
    }

    @Override
    public String toString() {
        return "ScanOptions[formats=" + formats + "]";
    }
}
