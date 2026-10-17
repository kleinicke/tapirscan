package org.tapirscan.android;

import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.Collections;
import java.util.List;
import java.util.Optional;

/** Structured content of decoded text, with fields in payload order. */
public final class Content {
    static {
        System.loadLibrary("tapirscan_jni");
    }

    private final ContentKind kind;
    private final List<ContentField> fields;

    private Content(ContentKind kind, List<ContentField> fields) {
        this.kind = kind;
        this.fields = Collections.unmodifiableList(fields);
    }

    /** Recognized payload kind. */
    public ContentKind kind() {
        return kind;
    }

    /** Named values in payload order. */
    public List<ContentField> fields() {
        return fields;
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
        return Collections.unmodifiableList(values);
    }

    static Optional<Content> parse(String text, boolean gs1) {
        byte[][] parts = nativeParse(text.getBytes(StandardCharsets.UTF_8), gs1);
        if (parts == null) return Optional.empty();
        List<ContentField> fields = new ArrayList<>();
        for (int i = 1; i + 1 < parts.length; i += 2) {
            fields.add(new ContentField(new String(parts[i], StandardCharsets.UTF_8),
                    new String(parts[i + 1], StandardCharsets.UTF_8)));
        }
        return Optional.of(new Content(ContentKind.fromName(new String(parts[0], StandardCharsets.UTF_8)), fields));
    }

    @Override
    public String toString() {
        return kind + fields.toString();
    }

    private static native byte[][] nativeParse(byte[] text, boolean gs1);
}
