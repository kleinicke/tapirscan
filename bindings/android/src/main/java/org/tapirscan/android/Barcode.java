package org.tapirscan.android;

/** One decoded barcode: text, format name and four corners in source-image pixels. */
public final class Barcode {
    private final String text;
    private final String format;
    private final double[] polygon;
    private final long support;

    Barcode(String text, String format, double[] polygon, long support) {
        this.text = text;
        this.format = format;
        this.polygon = polygon;
        this.support = support;
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

    @Override
    public String toString() {
        return format + ": " + text;
    }
}
