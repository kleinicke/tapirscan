package org.tapirscan.android;

import java.util.ArrayList;
import java.util.Collections;
import java.util.List;
import java.util.Optional;

/** A localized region without an accepted decode; a candidate, not a proven barcode. */
public final class UndecodedRegion {
    private final Optional<Format> format;
    private final List<Point> polygon;

    UndecodedRegion(Optional<Format> format, List<Point> polygon) {
        this.format = format;
        this.polygon = Collections.unmodifiableList(new ArrayList<>(polygon));
    }

    /** The reader that proposed the region; empty when unknown. */
    public Optional<Format> format() {
        return format;
    }

    /** Four corners in source-image pixels. */
    public List<Point> polygon() {
        return polygon;
    }

    @Override
    public boolean equals(Object other) {
        return other instanceof UndecodedRegion && format.equals(((UndecodedRegion) other).format)
                && polygon.equals(((UndecodedRegion) other).polygon);
    }

    @Override
    public int hashCode() {
        return format.hashCode() * 31 + polygon.hashCode();
    }

    @Override
    public String toString() {
        return "UndecodedRegion[format=" + format + ", polygon=" + polygon + "]";
    }
}
