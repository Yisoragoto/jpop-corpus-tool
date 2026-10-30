import XCTest

@testable import JPCore

/// 逐字行的振假名：读音压在对应的字上、单位照样各自能点、注音跨单位时不在中间换行
final class TimedRubyTests: XCTestCase {
    private func unit(_ start: Int, _ end: Int, _ sec: Double) -> LyricTimingUnit {
        LyricTimingUnit(charStart: start, charEnd: end, startSec: sec, endSec: sec + 1, source: "lrc")
    }

    func testNoRubiesKeepsThePlainUnitButtons() {
        XCTAssertNil(TimedRuby.clusters(text: "夜に", units: [unit(0, 2, 0)], rubies: []))
        // 落在原文外的注音不算
        XCTAssertNil(TimedRuby.clusters(text: "夜に", units: [unit(0, 2, 0)], rubies: [Ruby(start: 1, end: 5, reading: "x")]))
    }

    /// 注音只占单位的一部分：「夜に」一个单位，「夜」注よる。单位切成两段，遮罩按字数比例分
    func testARubyInsideOneUnitSplitsTheUnitProportionally() throws {
        let clusters = try XCTUnwrap(TimedRuby.clusters(text: "夜に", units: [unit(0, 2, 0)],
                                                        rubies: [Ruby(start: 0, end: 1, reading: "よる")]))
        XCTAssertEqual(clusters.count, 1)
        let pieces = clusters[0].pieces
        XCTAssertEqual(pieces.map(\.reading), ["よる", nil])
        XCTAssertEqual(pieces.map { $0.segments.map(\.text) }, [["夜"], ["に"]])
        let first = pieces[0].segments[0], second = pieces[1].segments[0]
        XCTAssertEqual(first.unit, 0)
        XCTAssertEqual(second.unit, 0)
        // 单位唱到一半：前一个字全亮，后一个字还没亮，和整个单位一个遮罩一样
        XCTAssertEqual(first.progress(unitProgress: 0.5), 1)
        XCTAssertEqual(second.progress(unitProgress: 0.5), 0)
        XCTAssertEqual(second.progress(unitProgress: 0.75), 0.5, accuracy: 1e-9)
    }

    /// 整词注音跨三个单位：「真夜中」注まよなか，单位一个字一个。三个单位进同一块（不在中间换行），
    /// 同一片上面一段读音，下面三段各属于自己的单位
    func testARubyAcrossUnitsKeepsThemTogether() throws {
        let units = [unit(0, 1, 0), unit(1, 2, 1), unit(2, 3, 2), unit(3, 4, 3)]
        let clusters = try XCTUnwrap(TimedRuby.clusters(text: "真夜中に", units: units,
                                                        rubies: [Ruby(start: 0, end: 3, reading: "まよなか")]))
        XCTAssertEqual(clusters.count, 2)
        XCTAssertEqual(clusters[0].pieces.count, 1)
        XCTAssertEqual(clusters[0].pieces[0].reading, "まよなか")
        XCTAssertEqual(clusters[0].pieces[0].segments.map(\.unit), [0, 1, 2])
        XCTAssertEqual(clusters[0].pieces[0].segments.map(\.text), ["真", "夜", "中"])
        XCTAssertEqual(clusters[1].pieces.map(\.reading), [nil])
        XCTAssertEqual(clusters[1].pieces[0].segments.map(\.text), ["に"])
        // 整个单位一段：进度就是单位的进度
        XCTAssertEqual(clusters[0].pieces[0].segments[1].progress(unitProgress: 0.3), 0.3, accuracy: 1e-9)
    }

    /// 码点下标（和 Rust 的 chars 一致）：前面有补充平面的字也对得上
    func testIndicesAreUnicodeScalars() throws {
        let clusters = try XCTUnwrap(TimedRuby.clusters(text: "😀光", units: [unit(0, 1, 0), unit(1, 2, 1)],
                                                        rubies: [Ruby(start: 1, end: 2, reading: "ひかり")]))
        XCTAssertEqual(clusters.flatMap { $0.pieces.map(\.reading) }, [nil, "ひかり"])
        XCTAssertEqual(clusters.last?.pieces.first?.segments.first?.text, "光")
    }
}
