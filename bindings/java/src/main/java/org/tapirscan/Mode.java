package org.tapirscan;

/** Work effort. {@link #MEDIUM} is the default. */
public enum Mode {
    /** Least work, intended for clear images and inexpensive retries. */
    LOW(0, "low"),
    /** Default balance of work and recovery. */
    MEDIUM(1, "medium"),
    /** Additional effort and source-detail recovery. */
    HIGH(2, "high"),
    /** Highest available effort, including an additional localization grid. */
    VERY_HIGH(3, "very-high");

    private final int code;
    private final String schemaName;

    Mode(int code, String schemaName) {
        this.code = code;
        this.schemaName = schemaName;
    }

    /** Native {@code TAPIRSCAN_MODE_*} value. */
    int code() {
        return code;
    }

    static Mode fromCode(int code) {
        for (Mode mode : values()) if (mode.code == code) return mode;
        throw new IllegalStateException("Unknown native mode " + code);
    }

    /** Stable name shared with the other language bindings, such as {@code "very-high"}. */
    @Override
    public String toString() {
        return schemaName;
    }
}
