package org.tapirscan.android;

import java.util.List;
import java.util.Optional;

/** Content parsing. Create a {@link Scanner} to scan images. */
public final class Tapirscan {
    private Tapirscan() {}

    /**
     * Structured content of decoded text, or empty for plain text. {@code gs1} parses the
     * text as a GS1 element string. See docs/CONTENT.md.
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
