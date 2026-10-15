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
     * Recognize GS1 element strings, URLs, Wi-Fi, contacts, payment slips, HIBC, VINs and
     * AAMVA licenses in decoded text. {@code gs1} parses the text as a GS1 element string.
     * Returns empty for text without recognized structure.
     */
    public static Optional<Content> parseContent(String text, boolean gs1) {
        return Content.parse(text, gs1);
    }

    /** Structured content of a decoded barcode, using its text and GS1 flag. */
    public static Optional<Content> parseContent(Barcode barcode) {
        return Content.parse(barcode.text(), barcode.gs1().orElse(false));
    }

    static Optional<Barcode> highestSupport(List<Barcode> barcodes) {
        Barcode winner = null;
        for (Barcode barcode : barcodes) {
            if (winner == null || barcode.support() > winner.support()) winner = barcode;
        }
        return Optional.ofNullable(winner);
    }
}
