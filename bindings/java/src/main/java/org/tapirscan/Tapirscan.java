package org.tapirscan;

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
}
