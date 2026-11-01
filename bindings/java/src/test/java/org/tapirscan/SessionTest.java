package org.tapirscan;

/** Camera-independent workflow contract; also compiled with the Android Java 11 target. */
public final class SessionTest {
    private static void check(boolean value) {
        if (!value) throw new AssertionError("session contract failed");
    }
    public static void main(String[] args) {
        ScanSession single = new ScanSession();
        check(!single.undo());
        check(single.accept(Format.QR_CODE, ""));
        check(single.finished() && !single.canUndo());
        check(!single.accept(Format.QR_CODE, "other") && !single.undo());
        single.reset();
        check(!single.finished() && single.entries().isEmpty());
        ScanSession list = new ScanSession(ScanSession.Mode.LIST);
        check(list.accept(Format.QR_CODE, "a\0é"));
        check(!list.accept(Format.QR_CODE, "a\0é"));
        var snapshot = list.entries();
        check(list.accept(Format.QR_CODE, "a\0é", true));
        check(snapshot.get(0).quantity() == 1 && list.entries().get(0).quantity() == 2);
        check(list.accept(Format.EAN13, "a\0é"));
        check(list.undo() && list.entries().size() == 1);
        check(list.undo() && list.entries().get(0).quantity() == 1);
        check(list.undo() && !list.undo());
        check(list.accept(Format.QR_CODE, "é"));
        check(list.accept(Format.QR_CODE, "e\u0301"));
        check(list.accept(Format.QR_CODE, " é"));
        var done = list.finish();
        check(done.size() == 3 && !list.undo() && !list.accept(Format.QR_CODE, "new"));
        try { done.clear(); throw new AssertionError("mutable snapshot"); }
        catch (UnsupportedOperationException expected) { /* immutable */ }
        list.reset();
        check(done.size() == 3 && list.entries().isEmpty() && list.mode() == ScanSession.Mode.LIST);
        list.finish();
        check(list.finished() && list.entries().isEmpty());
    }
}
