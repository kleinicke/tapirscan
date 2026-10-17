package org.tapirscan.android;

/** Kind of structured payload recognized by {@link Tapirscan#parseContent}. */
public enum ContentKind {
    /** GS1 element string or GS1 Digital Link; fields are named by application identifier. */
    GS1("GS1"),
    /** Web address. */
    URL("URL"),
    /** E-mail ({@code mailto:}, {@code MATMSG:}). */
    EMAIL("Email"),
    /** Telephone number ({@code tel:}). */
    PHONE("Phone"),
    /** Text message ({@code SMSTO:}, {@code sms:}). */
    SMS("SMS"),
    /** Geographic location ({@code geo:}). */
    GEO("Geo"),
    /** Wi-Fi network configuration ({@code WIFI:}). */
    WIFI("WiFi"),
    /** Contact card (vCard or {@code MECARD:}). */
    CONTACT("Contact"),
    /** Calendar event (iCalendar {@code VEVENT}). */
    CALENDAR("Calendar"),
    /** Swiss QR-bill payment part. Experimental: may change in minor releases. */
    SWISS_QR_BILL("SwissQRBill"),
    /** EPC/SEPA credit transfer. Experimental: may change in minor releases. */
    EPC_PAYMENT("EPCPayment"),
    /** Health Industry Bar Code. Experimental: may change in minor releases. */
    HIBC("HIBC"),
    /** Vehicle identification number with a valid check digit. Experimental: may change in minor releases. */
    VIN("VIN"),
    /** AAMVA driver's license or identification card. Experimental: may change in minor releases. */
    AAMVA("AAMVA");

    private final String schemaName;

    ContentKind(String schemaName) {
        this.schemaName = schemaName;
    }

    /** Stable name shared with the other language bindings, such as {@code "WiFi"}. */
    @Override
    public String toString() {
        return schemaName;
    }

    static ContentKind fromName(String name) {
        for (ContentKind kind : values()) if (kind.schemaName.equals(name)) return kind;
        throw new IllegalStateException("Unknown native content kind " + name);
    }
}
