package org.tapirscan;

/**
 * One named value of parsed content. Names repeat where the payload repeats them.
 *
 * @param name application identifier for GS1, otherwise a camelCase name such as {@code "ssid"}
 * @param value unescaped value; dates stay unconverted
 */
public record ContentField(String name, String value) {}
