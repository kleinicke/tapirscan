package org.tapirscan;

import java.util.List;
import java.util.Optional;

/**
 * A decoded physical instance. Equal values at distinct locations remain separate.
 *
 * @param support uncalibrated, reader-specific evidence used by {@link ScanResult#best()}
 * @param payloadBytes original decoded bytes where the reader reports them
 */
public record Barcode(
        String text,
        Format format,
        List<Point> polygon,
        long support,
        Optional<byte[]> payloadBytes,
        Optional<String> eanAddOn,
        Optional<Boolean> gs1,
        Optional<Boolean> readerInitialization,
        Optional<StructuredAppend> structuredAppend) {
    public Barcode {
        polygon = List.copyOf(polygon);
        payloadBytes = payloadBytes.map(byte[]::clone);
    }

    @Override
    public Optional<byte[]> payloadBytes() {
        return payloadBytes.map(byte[]::clone);
    }

    /** Enclosing integer pixel bounds, from floor(minimum) to ceil(maximum). */
    public Rect rect() {
        double left = Double.POSITIVE_INFINITY, top = Double.POSITIVE_INFINITY;
        double right = Double.NEGATIVE_INFINITY, bottom = Double.NEGATIVE_INFINITY;
        for (Point p : polygon) {
            left = Math.min(left, p.x());
            top = Math.min(top, p.y());
            right = Math.max(right, p.x());
            bottom = Math.max(bottom, p.y());
        }
        return new Rect(Math.floor(left), Math.floor(top),
                Math.ceil(right) - Math.floor(left), Math.ceil(bottom) - Math.floor(top));
    }
}
