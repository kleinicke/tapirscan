package org.tapirscan;

/** Kind of structured payload recognized by {@link Tapirscan#parseContent}. */
public enum ContentKind {
    /** GS1 element string or GS1 Digital Link; fields are named by application identifier. */
    GS1(1, "GS1"),
    /** Web address. */
    URL(2, "URL"),
    /** E-mail ({@code mailto:}, {@code MATMSG:}). */
    EMAIL(3, "Email"),
    /** Telephone number ({@code tel:}). */
    PHONE(4, "Phone"),
    /** Text message ({@code SMSTO:}, {@code sms:}). */
    SMS(5, "SMS"),
    /** Geographic location ({@code geo:}). */
    GEO(6, "Geo"),
    /** Wi-Fi network configuration ({@code WIFI:}). */
    WIFI(7, "WiFi"),
    /** Contact card (vCard or {@code MECARD:}). */
    CONTACT(8, "Contact"),
    /** Calendar event (iCalendar {@code VEVENT}). */
    CALENDAR(9, "Calendar"),
    /** Swiss QR-bill payment part. Experimental: may change in minor releases. */
    SWISS_QR_BILL(10, "SwissQRBill"),
    /** EPC/SEPA credit transfer. Experimental: may change in minor releases. */
    EPC_PAYMENT(11, "EPCPayment"),
    /** Health Industry Bar Code. Experimental: may change in minor releases. */
    HIBC(12, "HIBC"),
    /** Vehicle identification number with a valid check digit. Experimental: may change in minor releases. */
    VIN(13, "VIN"),
    /** AAMVA driver's license or identification card. Experimental: may change in minor releases. */
    AAMVA(14, "AAMVA");

    private final int code;
    private final String schemaName;

    ContentKind(int code, String schemaName) {
        this.code = code;
        this.schemaName = schemaName;
    }

    /** Native {@code tapirscan_content_kind} value. */
    public int code() {
        return code;
    }

    /** Stable name shared with the other language bindings, such as {@code "WiFi"}. */
    @Override
    public String toString() {
        return schemaName;
    }

    static ContentKind fromCode(int code) {
        for (ContentKind kind : values()) if (kind.code == code) return kind;
        throw new IllegalStateException("Unknown native content kind " + code);
    }
}
