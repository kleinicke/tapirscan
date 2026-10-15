package org.tapirscan.android;

import java.nio.charset.StandardCharsets;

/** One decoded barcode: text, format name and four corners in source-image pixels. */
public final class Barcode {
    private final String text;
    private final String format;
    private final double[] polygon;
    private final long support;
    private final Boolean gs1;

    /**
     * Called from JNI with the UTF-8 text bytes (JNI strings use modified UTF-8) and the
     * tri-state GS1 flag: -1 unreported, 0 or 1.
     */
    Barcode(byte[] text, String format, double[] polygon, long support, int gs1) {
        this.text = new String(text, StandardCharsets.UTF_8);
        this.format = format;
        this.polygon = polygon;
        this.support = support;
        this.gs1 = gs1 < 0 ? null : gs1 != 0;
    }

    /** Decoded text. */
    public String text() {
        return text;
    }

    /** Format name such as {@code "EAN13"} or {@code "QRCode"}. */
    public String format() {
        return format;
    }

    /** Corners as x0, y0, x1, y1, x2, y2, x3, y3 (a copy). */
    public double[] polygon() {
        return polygon.clone();
    }

    /** Uncalibrated, reader-specific ranking evidence. */
    public long support() {
        return support;
    }

    /** Whether the reader identified GS1 data, or {@code null} when it does not report it. */
    public Boolean gs1() {
        return gs1;
    }

    /**
     * Structured content of the text, such as GS1 elements or a Wi-Fi network, or
     * {@code null} for plain text.
     */
    public Content content() {
        return Content.parse(text, Boolean.TRUE.equals(gs1));
    }

    @Override
    public String toString() {
        return format + ": " + text;
    }
}
