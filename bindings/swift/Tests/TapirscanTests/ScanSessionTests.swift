import XCTest
@testable import Tapirscan

final class ScanSessionTests: XCTestCase {
    func testSingle() {
        let s = ScanSession()
        XCTAssertFalse(s.undo())
        XCTAssertTrue(s.accept(format: .qrCode, text: ""))
        XCTAssertTrue(s.finished)
        XCTAssertFalse(s.canUndo)
        XCTAssertFalse(s.accept(format: .qrCode, text: "next"))
        XCTAssertFalse(s.undo())
        s.reset()
        XCTAssertFalse(s.finished)
        XCTAssertTrue(s.entries.isEmpty)
    }
    func testList() {
        let s = ScanSession(mode: .list)
        XCTAssertTrue(s.accept(format: .qrCode, text: "a\0é"))
        XCTAssertFalse(s.accept(format: .qrCode, text: "a\0é"))
        let snapshot = s.entries
        XCTAssertTrue(s.accept(format: .qrCode, text: "a\0é", repeat: true))
        XCTAssertEqual(snapshot[0].quantity, 1)
        XCTAssertEqual(s.entries[0].quantity, 2)
        XCTAssertTrue(s.accept(format: .ean13, text: "a\0é"))
        XCTAssertTrue(s.undo())
        XCTAssertEqual(s.entries.count, 1)
        XCTAssertTrue(s.undo())
        XCTAssertEqual(s.entries[0].quantity, 1)
        XCTAssertTrue(s.undo())
        XCTAssertFalse(s.undo())
        for text in ["é", "e\u{0301}", " é"] {
            XCTAssertTrue(s.accept(format: .qrCode, text: text))
        }
        let done = s.finish()
        XCTAssertEqual(done.count, 3)
        XCTAssertFalse(s.undo())
        XCTAssertFalse(s.accept(format: .qrCode, text: "new"))
        s.reset()
        XCTAssertEqual(done.count, 3)
        XCTAssertTrue(s.entries.isEmpty)
        XCTAssertEqual(s.mode, .list)
        XCTAssertTrue(s.finish().isEmpty)
        XCTAssertTrue(s.finished)
    }
}
