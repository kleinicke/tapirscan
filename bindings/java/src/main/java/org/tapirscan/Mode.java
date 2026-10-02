package org.tapirscan;

/** Work effort. {@link #MEDIUM} is the default. */
public enum Mode {
    /** Least work, intended for clear images and inexpensive retries. */
    LOW("low"),
    /** Default balance of work and recovery. */
    MEDIUM("medium"),
    /** Additional effort and source-detail recovery. */
    HIGH("high"),
    /** Highest available effort, including an additional localization grid. */
    VERY_HIGH("very-high");

    private final String schemaName;

    Mode(String schemaName) {
        this.schemaName = schemaName;
    }

    /** Stable name shared with the other language bindings, such as {@code "very-high"}. */
    @Override
    public String toString() {
        return schemaName;
    }
}
