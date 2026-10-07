package org.tapirscan;

import java.util.Arrays;
import java.util.Objects;
import java.util.List;
import java.util.Optional;

/**
 * A decoded physical instance. Equal values at distinct locations remain separate.
 *
 * @param support uncalibrated, reader-specific evidence used by {@code best()}
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

    @Override
    public boolean equals(Object other) {
        if (this == other) return true;
        if (!(other instanceof Barcode b)) return false;
        return support == b.support && Objects.equals(text, b.text) && format == b.format
                && polygon.equals(b.polygon)
                && Arrays.equals(payloadBytes.orElse(null), b.payloadBytes.orElse(null))
                && eanAddOn.equals(b.eanAddOn) && gs1.equals(b.gs1)
                && readerInitialization.equals(b.readerInitialization)
                && structuredAppend.equals(b.structuredAppend);
    }

    @Override
    public int hashCode() {
        return Objects.hash(text, format, polygon, support,
                Arrays.hashCode(payloadBytes.orElse(null)), eanAddOn, gs1,
                readerInitialization, structuredAppend);
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
