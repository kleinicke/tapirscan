package org.tapirscan;

import java.util.List;
import java.util.Optional;

/** Decoded values and their source-image locations, valid after scanner closure. */
public record ScanResult(List<Barcode> barcodes) {
    public ScanResult { barcodes = List.copyOf(barcodes); }

    /** Decoded text in scanner order, including repeated values. */
    public List<String> values() { return barcodes.stream().map(Barcode::text).toList(); }

    /** Highest support, keeping first-read ties; empty when nothing was decoded. */
    public Optional<Barcode> best() { return Tapirscan.highestSupport(barcodes); }
}
