package org.tapirscan.android;

import java.util.Objects;

/** One named value of parsed content. Names repeat where the payload repeats them. */
public final class ContentField {
    private final String name;
    private final String value;

    ContentField(String name, String value) {
        this.name = name;
        this.value = value;
    }

    /** Application identifier for GS1, otherwise a camelCase name such as {@code "ssid"}. */
    public String name() {
        return name;
    }

    /** Unescaped value; dates stay unconverted. */
    public String value() {
        return value;
    }

    @Override
    public boolean equals(Object other) {
        return other instanceof ContentField && name.equals(((ContentField) other).name)
                && value.equals(((ContentField) other).value);
    }

    @Override
    public int hashCode() {
        return Objects.hash(name, value);
    }

    @Override
    public String toString() {
        return name + "=" + value;
    }
}
