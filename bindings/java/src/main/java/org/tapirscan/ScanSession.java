package org.tapirscan;

import java.util.ArrayList;
import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Objects;

/** Preview acceptance workflow. Use one thread per session; owns no camera or native resources. */
public final class ScanSession {
    /** Single finishes on first acceptance; list finishes explicitly. */
    public enum Mode { SINGLE, LIST }

    /** Immutable accepted identity and quantity, not a physical-object count. */
    public static final class Entry {
        private final Format format;
        private final String text;
        private final long quantity;
        private Entry(Format format, String text, long quantity) {
            this.format = format;
            this.text = text;
            this.quantity = quantity;
        }
        public Format format() { return format; }
        public String text() { return text; }
        public long quantity() { return quantity; }
    }

    private final Mode mode;
    private boolean finished;
    // Keys preserve exact strings (including NUL) without delimiter encodings.
    private final LinkedHashMap<Key, Entry> entries = new LinkedHashMap<>();
    private final ArrayList<Key> history = new ArrayList<>();

    private static final class Key {
        private final Format format;
        private final String text;
        private Key(Format format, String text) { this.format = format; this.text = text; }
        @Override public boolean equals(Object value) {
            if (!(value instanceof Key)) return false;
            Key other = (Key) value;
            return format == other.format && text.equals(other.text);
        }
        @Override public int hashCode() { return Objects.hash(format, text); }
    }

    public ScanSession() { this(Mode.SINGLE); }
    public ScanSession(Mode mode) { this.mode = Objects.requireNonNull(mode, "mode"); }
    public Mode mode() { return mode; }
    public boolean finished() { return finished; }
    public boolean canUndo() { return !finished && !history.isEmpty(); }
    /** Immutable snapshot in first-acceptance order; independent of reset and future edits. */
    public List<Entry> entries() {
        return Collections.unmodifiableList(new ArrayList<>(entries.values()));
    }
    public boolean accept(Barcode barcode) {
        Objects.requireNonNull(barcode, "barcode");
        return accept(barcode.format(), barcode.text());
    }
    public boolean accept(Format format, String text) { return accept(format, text, false); }
    /** repeat=true deliberately adds a unit; never use it for every frame. */
    public boolean accept(Format format, String text, boolean repeat) {
        if (finished) return false;
        Key key = new Key(Objects.requireNonNull(format, "format"), Objects.requireNonNull(text, "text"));
        Entry previous = entries.get(key);
        if (previous != null && !repeat) return false;
        long quantity = previous == null ? 1 : previous.quantity + 1;
        if (quantity > 9007199254740991L) throw new ArithmeticException("quantity exceeds the cross-language safe integer range");
        entries.put(key, new Entry(format, text, quantity));
        history.add(key);
        if (mode == Mode.SINGLE) finished = true;
        return true;
    }
    /** Reverse the latest accepted unit; false when empty or finished. */
    public boolean undo() {
        if (!canUndo()) return false;
        Key key = history.remove(history.size() - 1);
        Entry entry = entries.get(key);
        if (entry.quantity == 1) entries.remove(key);
        else entries.put(key, new Entry(entry.format, entry.text, entry.quantity - 1));
        return true;
    }
    /** Lock acceptance; the caller must stop its camera separately. */
    public List<Entry> finish() { finished = true; return entries(); }
    /** Start an empty session in the same mode. Existing snapshots remain valid. */
    public void reset() { entries.clear(); history.clear(); finished = false; }
}
