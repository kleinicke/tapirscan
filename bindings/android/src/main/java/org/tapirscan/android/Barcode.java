package org.tapirscan.android;

import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Collections;
import java.util.List;
import java.util.Objects;
import java.util.Optional;
import java.util.OptionalInt;

/** A decoded physical instance. Equal values at distinct locations remain separate. */
public final class Barcode {
    private final String text;
    private final Format format;
    private final List<Point> polygon;
    private final long support;
    private final byte[] payloadBytes;
    private final String eanAddOn;
    private final Boolean gs1;
    private final Boolean readerInitialization;
    private final StructuredAppend structuredAppend;
    private final String codabarStartStop;

    /**
     * Called from JNI with UTF-8 byte arrays (JNI strings use modified UTF-8), {@code null}
     * for absent optional fields, tri-state flags of -1 (not reported), 0 or 1, and a
     * structured-append count of 0 when absent.
     */
    Barcode(byte[] text, int format, double[] polygon, long support, byte[] payloadBytes,
            byte[] eanAddOn, int gs1, int readerInitialization, long appendIndex,
            long appendCount, byte[] appendId, int appendParity, byte[] codabarStartStop) {
        this.text = new String(text, StandardCharsets.UTF_8);
        this.format = Format.fromBit(format);
        List<Point> corners = new ArrayList<>(4);
        for (int i = 0; i < 4; i++) corners.add(new Point(polygon[2 * i], polygon[2 * i + 1]));
        this.polygon = Collections.unmodifiableList(corners);
        this.support = support;
        this.payloadBytes = payloadBytes;
        this.eanAddOn = eanAddOn == null ? null : new String(eanAddOn, StandardCharsets.UTF_8);
        this.gs1 = gs1 < 0 ? null : gs1 != 0;
        this.readerInitialization = readerInitialization < 0 ? null : readerInitialization != 0;
        this.structuredAppend = appendCount == 0 ? null
                : new StructuredAppend(appendIndex, appendCount,
                        Optional.ofNullable(appendId).map(b -> new String(b, StandardCharsets.UTF_8)),
                        appendParity < 0 ? OptionalInt.empty() : OptionalInt.of(appendParity));
        this.codabarStartStop = codabarStartStop == null ? null
                : new String(codabarStartStop, StandardCharsets.US_ASCII);
    }

    /** Decoded text. */
    public String text() {
        return text;
    }

    public Format format() {
        return format;
    }

    /** Four corners in source-image pixels. */
    public List<Point> polygon() {
        return polygon;
    }

    /** Uncalibrated, reader-specific evidence used by {@code best()}. */
    public long support() {
        return support;
    }

    /** Original decoded bytes where the reader reports them (a copy). */
    public Optional<byte[]> payloadBytes() {
        return Optional.ofNullable(payloadBytes).map(byte[]::clone);
    }

    /** The adjacent EAN/UPC supplement, when read. */
    public Optional<String> eanAddOn() {
        return Optional.ofNullable(eanAddOn);
    }

    /** Whether the reader identified GS1 data; empty when it does not report it. */
    public Optional<Boolean> gs1() {
        return Optional.ofNullable(gs1);
    }

    /** Whether the symbol programs the reader; empty when the reader does not report it. */
    public Optional<Boolean> readerInitialization() {
        return Optional.ofNullable(readerInitialization);
    }

    public Optional<StructuredAppend> structuredAppend() {
        return Optional.ofNullable(structuredAppend);
    }

    /**
     * Codabar start and stop characters (A to D) in reading order, such as {@code "AB"};
     * {@link #text()} omits them. Empty for other formats.
     */
    public Optional<String> codabarStartStop() {
        return Optional.ofNullable(codabarStartStop);
    }

    /** Enclosing integer pixel bounds, from floor(minimum) to ceil(maximum). */
    public Rect rect() {
        double left = Double.POSITIVE_INFINITY;
        double top = Double.POSITIVE_INFINITY;
        double right = Double.NEGATIVE_INFINITY;
        double bottom = Double.NEGATIVE_INFINITY;
        for (Point p : polygon) {
            left = Math.min(left, p.x());
            top = Math.min(top, p.y());
            right = Math.max(right, p.x());
            bottom = Math.max(bottom, p.y());
        }
        // Casting from double saturates; subtracting in double avoids int overflow.
        int x = (int) Math.floor(left);
        int y = (int) Math.floor(top);
        return new Rect(x, y, (int) (Math.ceil(right) - x), (int) (Math.ceil(bottom) - y));
    }

    @Override
    public boolean equals(Object other) {
        if (this == other) return true;
        if (!(other instanceof Barcode)) return false;
        Barcode b = (Barcode) other;
        return support == b.support && text.equals(b.text) && format == b.format
                && polygon.equals(b.polygon) && Arrays.equals(payloadBytes, b.payloadBytes)
                && Objects.equals(eanAddOn, b.eanAddOn) && Objects.equals(gs1, b.gs1)
                && Objects.equals(readerInitialization, b.readerInitialization)
                && Objects.equals(structuredAppend, b.structuredAppend)
                && Objects.equals(codabarStartStop, b.codabarStartStop);
    }

    @Override
    public int hashCode() {
        return Objects.hash(text, format, polygon, support, Arrays.hashCode(payloadBytes), eanAddOn,
                gs1, readerInitialization, structuredAppend, codabarStartStop);
    }

    @Override
    public String toString() {
        return format + ": " + text;
    }
}
