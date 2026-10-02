package org.tapirscan;

import java.util.Objects;
import java.util.Set;

/** Scanner configuration; per-image settings live in {@link ScanOptions}. */
public record ScannerOptions(Mode mode, Set<Format> formats, EanAddOnPolicy eanAddOnPolicy) {
    public ScannerOptions {
        Objects.requireNonNull(mode, "mode");
        Objects.requireNonNull(eanAddOnPolicy, "eanAddOnPolicy");
        formats = Set.copyOf(formats);
        if (formats.isEmpty()) throw new IllegalArgumentException("formats must not be empty");
    }

    /** Medium effort, Retail formats and ignored supplements. */
    public static ScannerOptions defaults() {
        return new ScannerOptions(Mode.MEDIUM, Format.RETAIL, EanAddOnPolicy.IGNORE);
    }

    public ScannerOptions withMode(Mode value) {
        return new ScannerOptions(value, formats, eanAddOnPolicy);
    }

    public ScannerOptions withFormats(Set<Format> value) {
        return new ScannerOptions(mode, value, eanAddOnPolicy);
    }

    public ScannerOptions withEanAddOnPolicy(EanAddOnPolicy value) {
        return new ScannerOptions(mode, formats, value);
    }
}
