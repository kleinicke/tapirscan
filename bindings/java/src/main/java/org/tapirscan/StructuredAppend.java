package org.tapirscan;

import java.util.Optional;
import java.util.OptionalInt;

/** Multipart sequence metadata. The index is one-based; sequences are not assembled. */
public record StructuredAppend(long index, long count, Optional<String> id, OptionalInt parity) {}
