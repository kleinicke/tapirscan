package org.tapirscan.android;

import java.util.ArrayList;
import java.util.Collections;
import java.util.List;
import java.util.Optional;

/** Scan output plus unread regions and diagnostics; valid after the scanner is closed. */
public final class InspectionResult {
    private final List<Barcode> barcodes;
    private final List<UndecodedRegion> undecoded;
    private final int width;
    private final int height;
    private final Mode mode;
    private final double elapsedMs;
    private final String diagnostics;

    InspectionResult(List<Barcode> barcodes, List<UndecodedRegion> undecoded, int width, int height,
            Mode mode, double elapsedMs, String diagnostics) {
        this.barcodes = Collections.unmodifiableList(new ArrayList<>(barcodes));
        this.undecoded = Collections.unmodifiableList(new ArrayList<>(undecoded));
        this.width = width;
        this.height = height;
        this.mode = mode;
        this.elapsedMs = elapsedMs;
        this.diagnostics = diagnostics;
    }

    public List<Barcode> barcodes() {
        return barcodes;
    }

    /** Localized but unread regions. */
    public List<UndecodedRegion> undecoded() {
        return undecoded;
    }

    /** Width of the supplied image in pixels. */
    public int width() {
        return width;
    }

    /** Height of the supplied image in pixels. */
    public int height() {
        return height;
    }

    public Mode mode() {
        return mode;
    }

    public double elapsedMs() {
        return elapsedMs;
    }

    /** Unstable engine diagnostics as JSON text. */
    public String diagnostics() {
        return diagnostics;
    }

    /** Highest support, keeping the first read on ties; empty when nothing was decoded. */
    public Optional<Barcode> best() {
        return Tapirscan.highestSupport(barcodes);
    }

    /** Decoded text of every barcode, in scanner order. */
    public List<String> values() {
        List<String> values = new ArrayList<>(barcodes.size());
        for (Barcode barcode : barcodes) values.add(barcode.text());
        return Collections.unmodifiableList(values);
    }

    @Override
    public String toString() {
        return "InspectionResult[barcodes=" + barcodes + ", undecoded=" + undecoded.size() + ", " + width + "x"
                + height + ", " + mode + "]";
    }
}
