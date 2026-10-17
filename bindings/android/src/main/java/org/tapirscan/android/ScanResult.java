package org.tapirscan.android;

import java.util.ArrayList;
import java.util.Collections;
import java.util.List;
import java.util.Optional;

/** Decoded values and their source-image locations, valid after the scanner is closed. */
public final class ScanResult {
    private final List<Barcode> barcodes;

    ScanResult(List<Barcode> barcodes) {
        this.barcodes = Collections.unmodifiableList(new ArrayList<>(barcodes));
    }

    public List<Barcode> barcodes() {
        return barcodes;
    }

    /** Decoded text in scanner order, including repeated values. */
    public List<String> values() {
        List<String> values = new ArrayList<>(barcodes.size());
        for (Barcode barcode : barcodes) values.add(barcode.text());
        return Collections.unmodifiableList(values);
    }

    /** Highest support, keeping first-read ties; empty when nothing was decoded. */
    public Optional<Barcode> best() {
        return Tapirscan.highestSupport(barcodes);
    }

    @Override
    public boolean equals(Object other) {
        return other instanceof ScanResult && barcodes.equals(((ScanResult) other).barcodes);
    }

    @Override
    public int hashCode() {
        return barcodes.hashCode();
    }

    @Override
    public String toString() {
        return "ScanResult" + barcodes;
    }
}
