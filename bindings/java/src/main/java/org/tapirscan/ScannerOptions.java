package org.tapirscan;

import java.util.Objects;
import java.util.Set;

/**
 * Scanner configuration; per-image settings live in {@link ScanOptions}.
 *
 * <p>{@code parallel} runs one scan's formats on separate threads where the host allows it;
 * results are identical either way. Use {@code false} when you already run one scanner per core.
 * {@code null} follows the process default: on unless {@code TAPIRSCAN_THREADS} is 0 or 1.
 */
public record ScannerOptions(Mode mode, Set<Format> formats, EanAddOnPolicy eanAddOnPolicy, Boolean parallel) {
    public ScannerOptions {
        Objects.requireNonNull(mode, "mode");
        Objects.requireNonNull(eanAddOnPolicy, "eanAddOnPolicy");
        formats = Set.copyOf(formats);
        if (formats.isEmpty()) throw new IllegalArgumentException("formats must not be empty");
    }

    /** Scanner configuration with the process-default threading. */
    public ScannerOptions(Mode mode, Set<Format> formats, EanAddOnPolicy eanAddOnPolicy) {
        this(mode, formats, eanAddOnPolicy, null);
    }

    /** Medium effort, Retail formats, ignored supplements and the process-default threading. */
    public static ScannerOptions defaults() {
        return new ScannerOptions(Mode.MEDIUM, Format.RETAIL, EanAddOnPolicy.IGNORE, null);
    }

    public ScannerOptions withMode(Mode value) {
        return new ScannerOptions(value, formats, eanAddOnPolicy, parallel);
    }

    public ScannerOptions withFormats(Set<Format> value) {
        return new ScannerOptions(mode, value, eanAddOnPolicy, parallel);
    }

    public ScannerOptions withEanAddOnPolicy(EanAddOnPolicy value) {
        return new ScannerOptions(mode, formats, value, parallel);
    }

    /** Run one scan's formats on separate threads ({@code true}) or on the calling thread. */
    public ScannerOptions withParallel(boolean value) {
        return new ScannerOptions(mode, formats, eanAddOnPolicy, value);
    }
}
