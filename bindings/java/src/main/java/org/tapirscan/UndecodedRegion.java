package org.tapirscan;

import java.util.List;
import java.util.Optional;

/** A localized region without an accepted decode; a candidate, not a proven barcode. */
public record UndecodedRegion(Optional<Format> format, List<Point> polygon) {
    public UndecodedRegion {
        polygon = List.copyOf(polygon);
    }
}
