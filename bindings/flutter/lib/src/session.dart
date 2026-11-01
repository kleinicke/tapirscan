import 'format.g.dart';
import 'result.dart';

/// Preview workflow selection. Single finishes on first acceptance.
enum ScanSessionMode { single, list }

/// Accepted units of a format/text pair, not an automatic physical-object count.
final class ScanEntry {
  const ScanEntry._(this.format, this.text, this.quantity);
  final Format format;
  final String text;
  final int quantity;
}

/// Preview acceptance state, independent of cameras, isolates and native scanning.
/// Keep a session in one isolate. The application owns camera control and storage.
final class ScanSession {
  ScanSession({this.mode = ScanSessionMode.single});
  final ScanSessionMode mode;
  bool _finished = false;
  final Map<(Format, String), ScanEntry> _entries = {};
  final List<(Format, String)> _history = [];

  bool get finished => _finished;
  bool get canUndo => !_finished && _history.isNotEmpty;

  /// Immutable snapshot in first-acceptance order; later edits do not affect it.
  List<ScanEntry> get entries => List.unmodifiable(_entries.values);

  bool acceptBarcode(Barcode barcode, {bool repeat = false}) =>
      accept(barcode.format, barcode.text, repeat: repeat);

  /// Add one unit. Repeat acceptance must be an explicit user action.
  bool accept(Format format, String text, {bool repeat = false}) {
    if (_finished) return false;
    final key = (format, text);
    final previous = _entries[key];
    if (previous != null && !repeat) return false;
    final quantity = (previous?.quantity ?? 0) + 1;
    if (quantity > 9007199254740991) {
      throw StateError(
        'quantity exceeds the cross-language safe integer range',
      );
    }
    _entries[key] = ScanEntry._(format, text, quantity);
    _history.add(key);
    if (mode == ScanSessionMode.single) _finished = true;
    return true;
  }

  /// Reverse the latest accepted unit; false when empty or finished.
  bool undo() {
    if (!canUndo) return false;
    final key = _history.removeLast();
    final entry = _entries[key]!;
    if (entry.quantity == 1) {
      _entries.remove(key);
    } else {
      _entries[key] = ScanEntry._(entry.format, entry.text, entry.quantity - 1);
    }
    return true;
  }

  /// Lock acceptance and return a snapshot. Does not stop a camera.
  List<ScanEntry> finish() {
    _finished = true;
    return entries;
  }

  /// Begin an empty session in the same mode; old snapshots stay unchanged.
  void reset() {
    _entries.clear();
    _history.clear();
    _finished = false;
  }
}
