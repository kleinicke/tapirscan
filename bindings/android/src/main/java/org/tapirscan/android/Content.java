package org.tapirscan.android;

import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.Collections;
import java.util.List;

/** Structured content of decoded text, with fields in payload order. */
public final class Content {
    /** Kind of structured payload; {@link #toString()} gives the name shared by all bindings. */
    public enum Kind {
        GS1("GS1"), URL("URL"), EMAIL("Email"), PHONE("Phone"), SMS("SMS"), GEO("Geo"),
        WIFI("WiFi"), CONTACT("Contact"), CALENDAR("Calendar"), SWISS_QR_BILL("SwissQRBill"),
        EPC_PAYMENT("EPCPayment"), HIBC("HIBC"), VIN("VIN"), AAMVA("AAMVA");

        private final String schemaName;

        Kind(String schemaName) {
            this.schemaName = schemaName;
        }

        @Override
        public String toString() {
            return schemaName;
        }

        static Kind fromName(String name) {
            for (Kind kind : values()) if (kind.schemaName.equals(name)) return kind;
            throw new IllegalStateException("Unknown content kind " + name);
        }
    }

    /** One named value; names repeat where the payload repeats them. */
    public static final class Field {
        private final String name;
        private final String value;

        Field(String name, String value) {
            this.name = name;
            this.value = value;
        }

        /** GS1 application identifier such as {@code "01"}, otherwise a name such as {@code "ssid"}. */
        public String name() {
            return name;
        }

        public String value() {
            return value;
        }

        @Override
        public String toString() {
            return name + "=" + value;
        }
    }

    static {
        System.loadLibrary("tapirscan_jni");
    }

    private final Kind kind;
    private final List<Field> fields;

    private Content(Kind kind, List<Field> fields) {
        this.kind = kind;
        this.fields = Collections.unmodifiableList(fields);
    }

    /**
     * Recognize GS1 element strings, URLs, Wi-Fi, contacts, payment slips, HIBC, VINs and
     * AAMVA licenses in decoded text; {@code gs1} parses it as a GS1 element string.
     * Returns {@code null} for text without recognized structure.
     */
    public static Content parse(String text, boolean gs1) {
        byte[][] parts = nativeParse(text.getBytes(StandardCharsets.UTF_8), gs1);
        if (parts == null) return null;
        List<Field> fields = new ArrayList<>();
        for (int i = 1; i + 1 < parts.length; i += 2) {
            fields.add(new Field(new String(parts[i], StandardCharsets.UTF_8),
                    new String(parts[i + 1], StandardCharsets.UTF_8)));
        }
        return new Content(Kind.fromName(new String(parts[0], StandardCharsets.UTF_8)), fields);
    }

    public Kind kind() {
        return kind;
    }

    /** Fields in payload order (unmodifiable). */
    public List<Field> fields() {
        return fields;
    }

    /** First value of a field, or {@code null}. */
    public String get(String name) {
        for (Field field : fields) if (field.name.equals(name)) return field.value;
        return null;
    }

    /** Every value of a field, in payload order. */
    public List<String> getAll(String name) {
        List<String> values = new ArrayList<>();
        for (Field field : fields) if (field.name.equals(name)) values.add(field.value);
        return Collections.unmodifiableList(values);
    }

    @Override
    public String toString() {
        return kind + fields.toString();
    }

    private static native byte[][] nativeParse(byte[] text, boolean gs1);
}
