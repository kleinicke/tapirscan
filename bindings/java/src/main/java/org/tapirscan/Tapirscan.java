package org.tapirscan;

import java.lang.foreign.Arena;
import java.lang.foreign.FunctionDescriptor;
import java.lang.foreign.Linker;
import java.lang.foreign.MemorySegment;
import java.lang.foreign.SymbolLookup;
import java.lang.invoke.MethodHandle;
import java.nio.charset.StandardCharsets;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.Locale;
import java.util.Objects;
import java.util.Optional;
import static java.lang.foreign.ValueLayout.*;

/** JDK 22+ native scanner. Close the scanner; returned results own all their data. */
public final class Tapirscan implements AutoCloseable {
    public enum Mode { LOW, MEDIUM, HIGH, VERY_HIGH }
    public record ScanOptions(boolean multiple, boolean includeRegions, int formats, boolean finishCandidates) {
        public ScanOptions(boolean multiple, boolean includeRegions, int formats) { this(multiple, includeRegions, formats, false); }
        public ScanOptions(boolean multiple, boolean includeRegions) { this(multiple, includeRegions, 15); }
        public static ScanOptions defaults() { return new ScanOptions(true,false); }
    }
    public record Point(double x, double y) {}
    public record Barcode(String text, List<Point> polygon, long support, String format) {
        public Barcode { polygon = List.copyOf(polygon); }
    }
    /** json retains localization, candidate evidence, search windows and all work flags. */
    public record ScanResult(List<Barcode> barcodes, boolean unfinished,
                             boolean localizationLimited, String json) {
        public ScanResult { barcodes = List.copyOf(barcodes); }
        public Optional<Barcode> best() {
            Barcode best = null;
            for (Barcode b : barcodes) if (best == null || b.support() > best.support()) best = b;
            return Optional.ofNullable(best);
        }
    }
    public static final class ScannerException extends RuntimeException {
        private static final long serialVersionUID = 1L;
        public final int code;
        public ScannerException(int code) { super("Barcode scanner error " + code); this.code = code; }
    }
    private static final long MAX_BYTES = 128L * 1024 * 1024;
    private final Arena libraryArena;
    private final MethodHandle destroy, scan, info, read, copyText, copyJson, destroyResult;
    private long handle;
    private final Mode mode;

    // This binding intentionally loads the selected native scanner library.
    @SuppressWarnings("restricted")
    public Tapirscan(Path libraryDirectory, Mode mode) {
        this.mode = Objects.requireNonNull(mode);
        Objects.requireNonNull(libraryDirectory);
        libraryArena = Arena.ofShared();
        try {
            String os = System.getProperty("os.name").toLowerCase(Locale.ROOT);
            String prefix = os.contains("win") ? "" : "lib";
            String suffix = os.contains("win") ? ".dll" : os.contains("mac") ? ".dylib" : ".so";
            Path library = libraryDirectory.toAbsolutePath().resolve(prefix + "tapirscan_" + mode.name().toLowerCase(Locale.ROOT) + suffix);
            SymbolLookup symbols = SymbolLookup.libraryLookup(library, libraryArena);
            long abi = call(bind(symbols,"barcode_abi_version",FunctionDescriptor.of(JAVA_INT)));
            if (abi != 4)
                throw new IllegalStateException("Native ABI mismatch: expected 4, got " + abi + ". Rebuild the native libraries.");
            if (call(bind(symbols,"barcode_mode",FunctionDescriptor.of(JAVA_INT))) != mode.ordinal())
                throw new IllegalStateException("Native library mode mismatch");
            MethodHandle create = bind(symbols,"tapirscan_create",FunctionDescriptor.of(JAVA_INT,ADDRESS));
            destroy = bind(symbols,"tapirscan_destroy",FunctionDescriptor.of(JAVA_INT,JAVA_LONG));
            scan = bind(symbols,"barcode_scan_formats",FunctionDescriptor.of(JAVA_INT,JAVA_LONG,ADDRESS,JAVA_LONG,JAVA_LONG,JAVA_LONG,JAVA_INT,JAVA_LONG,JAVA_INT,JAVA_INT,ADDRESS));
            info = bind(symbols,"barcode_result_info",FunctionDescriptor.of(JAVA_INT,JAVA_LONG,ADDRESS));
            read = bind(symbols,"barcode_result_read",FunctionDescriptor.of(JAVA_INT,JAVA_LONG,JAVA_LONG,ADDRESS));
            copyText = bind(symbols,"barcode_result_copy_text",FunctionDescriptor.of(JAVA_INT,JAVA_LONG,JAVA_LONG,ADDRESS,JAVA_LONG));
            copyJson = bind(symbols,"barcode_result_copy_json",FunctionDescriptor.of(JAVA_INT,JAVA_LONG,ADDRESS,JAVA_LONG));
            destroyResult = bind(symbols,"barcode_result_destroy",FunctionDescriptor.of(JAVA_INT,JAVA_LONG));
            try (Arena arena = Arena.ofConfined()) {
                MemorySegment out = arena.allocate(JAVA_LONG);
                check(call(create,out)); handle = out.get(JAVA_LONG,0);
            }
        } catch (RuntimeException | Error e) { libraryArena.close(); throw e; }
    }
    // The declared descriptors match the version-checked C ABI.
    @SuppressWarnings("restricted")
    private static MethodHandle bind(SymbolLookup symbols, String name, FunctionDescriptor descriptor) {
        return Linker.nativeLinker().downcallHandle(symbols.find(name).orElseThrow(),descriptor);
    }
    private static int call(MethodHandle method, Object... args) {
        try { return (int)method.invokeWithArguments(args); }
        catch (RuntimeException | Error e) { throw e; }
        catch (Throwable e) { throw new IllegalStateException("Native call failed",e); }
    }
    private static void check(int status) { if (status != 0) throw new ScannerException(status); }
    public Mode mode() { return mode; }

    /** Pixels are gray8, RGB8 or RGBA8, never BGR/ARGB. Row stride is in bytes. */
    public synchronized ScanResult scan(byte[] pixels, int width, int height, int channels, long stride, ScanOptions options) {
        Objects.requireNonNull(options);
        if (handle == 0) throw new IllegalStateException("Scanner is closed");
        Objects.requireNonNull(pixels);
        if (width < 3 || height < 3 || (channels != 1 && channels != 3 && channels != 4))
            throw new IllegalArgumentException("Invalid dimensions or channels");
        long row = (long)width * channels;
        if (stride < row || stride > MAX_BYTES) throw new IllegalArgumentException("Invalid stride");
        long required = Math.addExact(Math.multiplyExact(height-1L,stride),row);
        if (required > MAX_BYTES || required > pixels.length) throw new IllegalArgumentException("Image buffer is too short or exceeds 128 MiB");
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment input = arena.allocate(required);
            input.copyFrom(MemorySegment.ofArray(pixels).asSlice(0,required));
            MemorySegment out = arena.allocate(JAVA_LONG);
            int flags=(options.multiple() ? 0 : 1) | (options.includeRegions() ? 2 : 0) | (options.finishCandidates() ? 16 : 0);
            check(call(scan,handle,input,required,(long)width,(long)height,channels,stride,flags,options.formats(),out));
            long result = out.get(JAVA_LONG,0);
            try {
                MemorySegment meta = arena.allocate(24,8);
                check(call(info,result,meta));
                long length = meta.get(JAVA_LONG,0), count = meta.get(JAVA_LONG,8);
                MemorySegment json = arena.allocate(Math.addExact(length,1));
                check(call(copyJson,result,json,length+1));
                String full = new String(json.asSlice(0,length).toArray(JAVA_BYTE),StandardCharsets.UTF_8);
                ArrayList<Barcode> barcodes = new ArrayList<>(Math.toIntExact(count));
                MemorySegment value = arena.allocate(104,8);
                for (long i=0;i<count;i++) {
                    check(call(read,result,i,value));
                    ArrayList<Point> polygon = new ArrayList<>(4);
                    for (int p=0;p<4;p++) polygon.add(new Point(value.get(JAVA_DOUBLE,p*16L),value.get(JAVA_DOUBLE,p*16L+8)));
                    long textLength=value.get(JAVA_LONG,72);
                    MemorySegment textBytes=arena.allocate(Math.addExact(textLength,1));
                    check(call(copyText,result,i,textBytes,textLength+1));
                    String text=new String(textBytes.asSlice(0,textLength).toArray(JAVA_BYTE),StandardCharsets.UTF_8);
                    String format=value.getString(80);
                    barcodes.add(new Barcode(text,polygon,Integer.toUnsignedLong(value.get(JAVA_INT,64)),format));
                }
                return new ScanResult(barcodes,meta.get(JAVA_INT,16)!=0,meta.get(JAVA_INT,20)!=0,full);
            } finally { check(call(destroyResult,result)); }
        }
    }
    public ScanResult scan(byte[] pixels, int width, int height, int channels, long stride) {
        return scan(pixels,width,height,channels,stride,ScanOptions.defaults());
    }
    public ScanResult scan(byte[] pixels, int width, int height, int channels, ScanOptions options) {
        return scan(pixels,width,height,channels,(long)width*channels,options);
    }
    public ScanResult scan(byte[] pixels, int width, int height, int channels) {
        return scan(pixels,width,height,channels,(long)width*channels);
    }
    @Override public synchronized void close() {
        if (handle != 0) {
            long old = handle; handle = 0;
            try { check(call(destroy,old)); } finally { libraryArena.close(); }
        }
    }
}
