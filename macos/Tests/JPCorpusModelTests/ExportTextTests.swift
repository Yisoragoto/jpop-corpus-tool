import Foundation
import XCTest
@testable import JPCorpusModel

final class ExportTextTests: XCTestCase {
    private func fixture() throws -> [String: Any] {
        let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        return try XCTUnwrap(JSONSerialization.jsonObject(with: Data(contentsOf: root.appendingPathComponent("app/src/testdata/python_formats.json"))) as? [String: Any])
    }
    // pyFormat.test.ts 全部 4 个原用例，直接读取同一份 Python fixture。
    func testFixedMatchesPythonFixturesAndEvenTies() throws {
        let rows = try XCTUnwrap(fixture()["fixed"] as? [[Any]])
        XCTAssertGreaterThan(rows.count, 2000)
        for row in rows { let value = try XCTUnwrap(Double(try XCTUnwrap(row[0] as? String))); let digits = try XCTUnwrap(row[1] as? Int); XCTAssertEqual(ExportText.pyFixed(value, digits: digits), row[2] as? String, "\(value) / \(digits)") }
        XCTAssertEqual([ExportText.pyFixed(0.125, digits: 2), ExportText.pyFixed(2.5, digits: 0), ExportText.pyFixed(3.5, digits: 0), ExportText.pyFixed(24.25, digits: 1)], ["0.12", "2", "4", "24.2"])
    }
    func testGroupedMatchesPythonFixtures() throws {
        for row in try XCTUnwrap(fixture()["grouped"] as? [[Any]]) { XCTAssertEqual(ExportText.pyGrouped(try XCTUnwrap(row[0] as? Double)), row[1] as? String) }
    }
    func testFloatRepresentation() {
        XCTAssertEqual([60, 0, 12.34, 5e-5, 0.1+0.2].map(ExportText.pyFloatRepr), ["60.0", "0.0", "12.34", "5e-05", "0.30000000000000004"])
    }
    func testKwicCsvMatchesPythonByteForByte() throws {
        let f = try fixture(); let data = try JSONSerialization.data(withJSONObject: XCTUnwrap(f["kwicRows"]))
        let rows = try JSONDecoder().decode([ExportText.KwicCsvHit].self, from: data)
        let actual = ExportText.kwicCsv(rows)
        let expected = try XCTUnwrap(f["kwicCsv"] as? String)
        XCTAssertTrue(actual.hasPrefix("\u{feff}"))
        // Foundation JSONSerialization 可能丢掉 fixture 字符串开头的 BOM。
        let expectedContent = expected.hasPrefix("\u{feff}") ? expected.dropFirst() : expected[...]
        XCTAssertEqual(Array(actual.dropFirst().utf8), Array(expectedContent.utf8))
    }
    func testFilenameAndEscaping() {
        XCTAssertEqual(ExportText.kwicFilename("(夜|空)"), "夜_空.csv")
        XCTAssertEqual(ExportText.kwicFilename(""), "kwic.csv")
        XCTAssertEqual(ExportText.kwicFilename("a/b:c"), "a_b_c.csv")
        XCTAssertEqual(ExportText.csvField("a\"b"), "\"a\"\"b\"")
        XCTAssertEqual(ExportText.csvField("a\rb"), "\"a\rb\"")
        XCTAssertEqual(ExportText.csvField("a\nb"), "\"a\nb\"")
        XCTAssertEqual(ExportText.csvRow([.text("a,b"), .empty, .number(60)]), "\"a,b\",,60.0\r\n")
    }
}
