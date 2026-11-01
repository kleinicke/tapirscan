import { test } from "node:test";
import assert from "node:assert/strict";
// Exercise the published package export, without initializing a scanner or camera.
import { ScanSession } from "tapirscan/session";

const a = { format: "EAN13", text: "4006381333931" };
const b = { format: "QRCode", text: "second" };

test("single acceptance locks the session until reset", () => {
  const session = new ScanSession();
  assert.equal(session.finished, false);
  assert.equal(session.accept(a), true);
  assert.equal(session.finished, true);
  assert.equal(session.accept(b), false);
  assert.equal(session.accept(a, { repeat: true }), false);
  assert.equal(session.undo(), false);
  assert.deepEqual(session.finish(), [{ ...a, quantity: 1 }]);
  session.reset();
  assert.equal(session.accept(b), true);
  assert.deepEqual(session.entries, [{ ...b, quantity: 1 }]);
});

test("list duplicates stay suppressed across intervening items; repeats are explicit", () => {
  const session = new ScanSession({ mode: "list" });
  assert.equal(session.accept(a), true);
  assert.equal(session.accept(a), false);
  assert.equal(session.accept(b), true);
  assert.equal(session.accept(a), false);
  assert.equal(session.accept(a, { repeat: true }), true);
  assert.deepEqual(session.entries, [
    { ...a, quantity: 2 },
    { ...b, quantity: 1 },
  ]);
  assert.equal(session.finished, false);
});

test("undo reverses successful units in order and allows a removed identity again", () => {
  const session = new ScanSession({ mode: "list" });
  assert.equal(session.undo(), false);
  session.accept(a);
  session.accept(b);
  session.accept(a, { repeat: true });
  session.accept(b); // A suppressed attempt must not enter the undo history.
  assert.equal(session.undo(), true);
  assert.deepEqual(session.entries, [
    { ...a, quantity: 1 },
    { ...b, quantity: 1 },
  ]);
  assert.equal(session.undo(), true);
  assert.equal(session.accept(b), true);
  assert.equal(session.undo(), true);
  assert.equal(session.undo(), true);
  assert.equal(session.canUndo, false);
  assert.deepEqual(session.entries, []);
});

test("finished snapshots cannot change through inputs, later actions or reset", () => {
  const session = new ScanSession({ mode: "list" });
  const mutable = { ...a };
  session.accept(mutable);
  const earlier = session.entries;
  mutable.text = "changed";
  session.accept(a, { repeat: true });
  const finished = session.finish();
  assert.equal(session.canUndo, false);
  assert.equal(session.accept(b), false);
  assert.throws(() => {
    finished[0].quantity = 9;
  }, TypeError);
  assert.throws(() => finished.push(b), TypeError);
  session.reset();
  session.accept(b);
  assert.deepEqual(earlier, [{ ...a, quantity: 1 }]);
  assert.deepEqual(finished, [{ ...a, quantity: 2 }]);
});

test("format and exact text define identity without trimming or delimiter collisions", () => {
  const session = new ScanSession({ mode: "list" });
  for (const item of [
    a,
    { ...a, format: "QRCode" },
    { ...a, text: ` ${a.text}` },
    { format: "QRCode", text: '\n["EAN13","x"]' },
    { format: "QRCode", text: "" },
  ]) {
    assert.equal(session.accept(item), true);
  }
  assert.equal(session.entries.length, 5);
});

test("invalid input fails before mutating the session; an empty list can finish", () => {
  assert.throws(() => new ScanSession({ mode: "count" }), TypeError);
  const session = new ScanSession({ mode: "list" });
  for (const invalid of [null, undefined, {}, { format: "", text: "x" }, { ...a, text: 42 }])
    assert.throws(() => session.accept(invalid), TypeError);
  assert.throws(() => session.accept({ format: "Unknown", text: "x" }), TypeError);
  assert.throws(() => session.accept(a, { repeat: "false" }), TypeError);
  assert.deepEqual(session.entries, []);
  assert.deepEqual(session.finish(), []);
  assert.equal(session.finished, true);
});
