package org.tapirscan.android;

import java.util.Collections;
import java.util.EnumSet;
import java.util.Objects;
import java.util.Set;

/**
 * Scanner configuration; per-image settings live in {@link ScanOptions}.
 *
 * <p>{@code parallel} runs one scan's formats on separate threads; results are identical
 * either way. {@code null} follows the process default: on unless {@code TAPIRSCAN_THREADS}
 * is 0 or 1.
 */
public final class ScannerOptions {
    private final Mode mode;
    private final Set<Format> formats;
    private final EanAddOnPolicy eanAddOnPolicy;
    private final Boolean parallel;

    public ScannerOptions(
            Mode mode, Set<Format> formats, EanAddOnPolicy eanAddOnPolicy, Boolean parallel) {
        this.mode = Objects.requireNonNull(mode, "mode");
        this.eanAddOnPolicy = Objects.requireNonNull(eanAddOnPolicy, "eanAddOnPolicy");
        this.formats = copy(formats);
        this.parallel = parallel;
    }

    /** Scanner configuration with the process-default threading. */
    public ScannerOptions(Mode mode, Set<Format> formats, EanAddOnPolicy eanAddOnPolicy) {
        this(mode, formats, eanAddOnPolicy, null);
    }

    /** Medium effort, Retail formats, ignored supplements and the process-default threading. */
    public static ScannerOptions defaults() {
        return new ScannerOptions(Mode.MEDIUM, Format.RETAIL, EanAddOnPolicy.IGNORE, null);
    }

    public Mode mode() {
        return mode;
    }

    public Set<Format> formats() {
        return formats;
    }

    public EanAddOnPolicy eanAddOnPolicy() {
        return eanAddOnPolicy;
    }

    /** {@code null} when the process default applies. */
    public Boolean parallel() {
        return parallel;
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

    /** An unmodifiable copy; an empty selection is rejected. */
    static Set<Format> copy(Set<Format> formats) {
        if (formats.isEmpty()) throw new IllegalArgumentException("formats must not be empty");
        return Collections.unmodifiableSet(EnumSet.copyOf(formats));
    }

    @Override
    public boolean equals(Object other) {
        if (!(other instanceof ScannerOptions)) return false;
        ScannerOptions o = (ScannerOptions) other;
        return mode == o.mode
                && formats.equals(o.formats)
                && eanAddOnPolicy == o.eanAddOnPolicy
                && Objects.equals(parallel, o.parallel);
    }

    @Override
    public int hashCode() {
        return Objects.hash(mode, formats, eanAddOnPolicy, parallel);
    }

    @Override
    public String toString() {
        return "ScannerOptions[mode=" + mode + ", formats=" + formats + ", eanAddOnPolicy=" + eanAddOnPolicy
                + ", parallel=" + parallel + "]";
    }
}
