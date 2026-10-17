package org.tapirscan.android;

import java.util.Collections;
import java.util.EnumSet;
import java.util.Objects;
import java.util.Set;

/** Scanner configuration; per-image settings live in {@link ScanOptions}. */
public final class ScannerOptions {
    private final Mode mode;
    private final Set<Format> formats;
    private final EanAddOnPolicy eanAddOnPolicy;

    public ScannerOptions(Mode mode, Set<Format> formats, EanAddOnPolicy eanAddOnPolicy) {
        this.mode = Objects.requireNonNull(mode, "mode");
        this.eanAddOnPolicy = Objects.requireNonNull(eanAddOnPolicy, "eanAddOnPolicy");
        this.formats = copy(formats);
    }

    /** Medium effort, Retail formats and ignored supplements. */
    public static ScannerOptions defaults() {
        return new ScannerOptions(Mode.MEDIUM, Format.RETAIL, EanAddOnPolicy.IGNORE);
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

    public ScannerOptions withMode(Mode value) {
        return new ScannerOptions(value, formats, eanAddOnPolicy);
    }

    public ScannerOptions withFormats(Set<Format> value) {
        return new ScannerOptions(mode, value, eanAddOnPolicy);
    }

    public ScannerOptions withEanAddOnPolicy(EanAddOnPolicy value) {
        return new ScannerOptions(mode, formats, value);
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
        return mode == o.mode && formats.equals(o.formats) && eanAddOnPolicy == o.eanAddOnPolicy;
    }

    @Override
    public int hashCode() {
        return Objects.hash(mode, formats, eanAddOnPolicy);
    }

    @Override
    public String toString() {
        return "ScannerOptions[mode=" + mode + ", formats=" + formats + ", eanAddOnPolicy=" + eanAddOnPolicy + "]";
    }
}
