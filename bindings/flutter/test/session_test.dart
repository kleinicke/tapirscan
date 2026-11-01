import 'package:tapirscan/tapirscan.dart';
import 'package:test/test.dart';

void main() {
  test('single locks on first acceptance and reset unlocks', () {
    final s = ScanSession();
    expect(s.undo(), isFalse);
    expect(s.accept(Format.qrCode, ''), isTrue);
    expect(s.finished, isTrue);
    expect(s.canUndo, isFalse);
    expect(s.accept(Format.qrCode, 'next'), isFalse);
    expect(s.undo(), isFalse);
    s.reset();
    expect(s.finished, isFalse);
    expect(s.entries, isEmpty);
  });
  test('list preserves identities, quantities, history and snapshots', () {
    final s = ScanSession(mode: ScanSessionMode.list);
    expect(s.accept(Format.qrCode, 'a\u0000é'), isTrue);
    expect(s.accept(Format.qrCode, 'a\u0000é'), isFalse);
    final snapshot = s.entries;
    expect(s.accept(Format.qrCode, 'a\u0000é', repeat: true), isTrue);
    expect(snapshot.single.quantity, 1);
    expect(s.entries.single.quantity, 2);
    expect(() => snapshot.clear(), throwsUnsupportedError);
    expect(s.accept(Format.ean13, 'a\u0000é'), isTrue);
    expect(s.undo(), isTrue);
    expect(s.entries.length, 1);
    expect(s.undo(), isTrue);
    expect(s.entries.single.quantity, 1);
    expect(s.undo(), isTrue);
    expect(s.undo(), isFalse);
    for (final text in ['é', 'e\u0301', ' é']) {
      expect(s.accept(Format.qrCode, text), isTrue);
    }
    final done = s.finish();
    expect(done.length, 3);
    expect(s.undo(), isFalse);
    expect(s.accept(Format.qrCode, 'new'), isFalse);
    s.reset();
    expect(done.length, 3);
    expect(s.entries, isEmpty);
    expect(s.mode, ScanSessionMode.list);
    expect(s.finish(), isEmpty);
    expect(s.finished, isTrue);
  });
}
