package org.tapirscan;

import static java.lang.foreign.ValueLayout.JAVA_BYTE;
import static java.lang.foreign.ValueLayout.JAVA_INT;
import static java.lang.foreign.ValueLayout.JAVA_LONG;

import java.lang.foreign.Arena;
import java.lang.foreign.MemorySegment;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.List;
import java.util.Optional;

/**
 * Structured content of decoded text, with fields in payload order.
 *
 * @param kind recognized payload kind
 * @param fields named values in payload order
 */
public record Content(ContentKind kind, List<ContentField> fields) {
    public Content {
        fields = List.copyOf(fields);
    }

    /** First value of a field. */
    public Optional<String> get(String name) {
        for (ContentField field : fields) if (field.name().equals(name)) return Optional.of(field.value());
        return Optional.empty();
    }

    /** Every value of a field, in payload order. */
    public List<String> getAll(String name) {
        List<String> values = new ArrayList<>();
        for (ContentField field : fields) if (field.name().equals(name)) values.add(field.value());
        return List.copyOf(values);
    }

    static Optional<Content> parse(String text, boolean gs1) {
        Native lib = Native.get();
        try (Arena arena = Arena.ofConfined()) {
            byte[] bytes = text.getBytes(StandardCharsets.UTF_8);
            MemorySegment input = arena.allocate(Math.max(1, bytes.length));
            MemorySegment.copy(bytes, 0, input, JAVA_BYTE, 0, bytes.length);
            MemorySegment handle = arena.allocate(JAVA_LONG);
            MemorySegment error = arena.allocate(Native.ERROR);
            lib.check(Native.call(lib.parseContent, input, (long) bytes.length, gs1 ? 1 : 0, handle, error), error);
            long content = handle.get(JAVA_LONG, 0);
            if (content == 0) return Optional.empty();
            try {
                MemorySegment summary = arena.allocate(Native.CONTENT_SUMMARY);
                lib.check(Native.call(lib.contentInfo, content, summary));
                ContentKind kind = ContentKind.fromCode(summary.get(JAVA_INT, 0));
                long count = summary.get(JAVA_LONG, Native.offset(Native.CONTENT_SUMMARY, "fieldCount"));
                List<ContentField> fields = new ArrayList<>();
                MemorySegment lengths = arena.allocate(Native.CONTENT_LENGTHS);
                for (long i = 0; i < count; i++) {
                    lib.check(Native.call(lib.contentField, content, i, lengths));
                    fields.add(new ContentField(
                            copy(lib, arena, content, i, 0, lengths.get(JAVA_LONG, 0)),
                            copy(lib, arena, content, i, 1, lengths.get(JAVA_LONG, 8))));
                }
                return Optional.of(new Content(kind, fields));
            } finally {
                Native.call(lib.contentDestroy, content);
            }
        }
    }

    private static String copy(Native lib, Arena arena, long content, long index, int part, long length) {
        MemorySegment out = arena.allocate(length + 1);
        lib.check(Native.call(lib.contentCopy, content, index, part, out, length + 1));
        return new String(out.asSlice(0, length).toArray(JAVA_BYTE), StandardCharsets.UTF_8);
    }
}
