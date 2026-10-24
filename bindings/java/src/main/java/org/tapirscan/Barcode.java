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
 * @param codabarStartStop Codabar start and stop characters (A to D) in reading order,
 *     such as {@code "AB"}, which {@code text} omits; empty for other formats
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
        Optional<StructuredAppend> structuredAppend,
        Optional<String> codabarStartStop) {
    public Barcode {
        polygon = List.copyOf(polygon);
        payloadBytes = payloadBytes.map(byte[]::clone);
    }

    /** A barcode without Codabar start and stop characters. */
    public Barcode(String text, Format format, List<Point> polygon, long support,
            Optional<byte[]> payloadBytes, Optional<String> eanAddOn, Optional<Boolean> gs1,
            Optional<Boolean> readerInitialization, Optional<StructuredAppend> structuredAppend) {
        this(text, format, polygon, support, payloadBytes, eanAddOn, gs1, readerInitialization,
                structuredAppend, Optional.empty());
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
                && structuredAppend.equals(b.structuredAppend)
                && codabarStartStop.equals(b.codabarStartStop);
    }

    @Override
    public int hashCode() {
        return Objects.hash(text, format, polygon, support,
                Arrays.hashCode(payloadBytes.orElse(null)), eanAddOn, gs1,
                readerInitialization, structuredAppend, codabarStartStop);
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
        // Casting from double saturates; subtracting in double avoids int overflow.
        int x = (int) Math.floor(left), y = (int) Math.floor(top);
        return new Rect(x, y, (int) (Math.ceil(right) - x), (int) (Math.ceil(bottom) - y));
    }
}
