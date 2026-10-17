package org.tapirscan.android;

import java.util.Objects;
import java.util.Optional;
import java.util.OptionalInt;

/** Multipart sequence metadata. The index is one-based; sequences are not assembled. */
public final class StructuredAppend {
    private final long index;
    private final long count;
    private final Optional<String> id;
    private final OptionalInt parity;

    StructuredAppend(long index, long count, Optional<String> id, OptionalInt parity) {
        this.index = index;
        this.count = count;
        this.id = id;
        this.parity = parity;
    }

    public long index() {
        return index;
    }

    public long count() {
        return count;
    }

    public Optional<String> id() {
        return id;
    }

    public OptionalInt parity() {
        return parity;
    }

    @Override
    public boolean equals(Object other) {
        if (!(other instanceof StructuredAppend)) return false;
        StructuredAppend s = (StructuredAppend) other;
        return index == s.index && count == s.count && id.equals(s.id) && parity.equals(s.parity);
    }

    @Override
    public int hashCode() {
        return Objects.hash(index, count, id, parity);
    }

    @Override
    public String toString() {
        return "StructuredAppend[index=" + index + ", count=" + count + ", id=" + id + ", parity=" + parity + "]";
    }
}
