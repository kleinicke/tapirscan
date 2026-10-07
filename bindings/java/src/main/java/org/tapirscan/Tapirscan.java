package org.tapirscan;

import java.util.List;
import java.util.Optional;

/**
 * One-shot scanning. Reuse a {@link Scanner} for many images.
 *
 * <p>The native library is located through the {@code tapirscan.library} system property
 * (a file), the {@code TAPIRSCAN_LIBRARY_DIR} environment variable (a directory), or the
 * operating system's library search path.
 */
public final class Tapirscan {
    private Tapirscan() {}

    /** Scan with Medium effort and Retail formats. */
    public static ScanResult scan(Image image) {
        return scan(image, ScannerOptions.defaults());
    }

    public static ScanResult scan(Image image, ScannerOptions options) {
        try (Scanner scanner = new Scanner(options)) {
            return scanner.scan(image);
        }
    }
    /** Inspect with Medium effort and Retail formats. */
    public static InspectionResult inspect(Image image) {
        return inspect(image, ScannerOptions.defaults());
    }

    public static InspectionResult inspect(Image image, ScannerOptions options) {
        try (Scanner scanner = new Scanner(options)) {
            return scanner.inspect(image);
        }
    }

    /**
     * Highest support, keeping the first read on ties; empty when there are no barcodes.
     * Works on {@code scan} output and inspection results alike. Support is uncalibrated
     * evidence, so select by format, payload or position when the application knows them.
     */
    public static Optional<Barcode> best(List<Barcode> barcodes) {
        Barcode winner = null;
        for (Barcode barcode : barcodes) {
            if (winner == null || barcode.support() > winner.support()) winner = barcode;
        }
        return Optional.ofNullable(winner);
    }
}
