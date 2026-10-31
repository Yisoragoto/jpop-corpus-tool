import Foundation
import XCTest

@testable import JPCore

/// Windows 的 karaokeStyle.test.ts 读同一份 Fixtures/KaraokeStyle.json：两端字段名、预设、校验规则必须一致
final class KaraokeStyleTests: XCTestCase {
    private func shared() throws -> [String: Any] {
        let url = try XCTUnwrap(Bundle.module.url(forResource: "KaraokeStyle", withExtension: "json", subdirectory: "Fixtures"))
        return try XCTUnwrap(JSONSerialization.jsonObject(with: Data(contentsOf: url)) as? [String: Any])
    }

    private func style(_ object: Any?) throws -> KaraokeStyle {
        let data = try JSONSerialization.data(withJSONObject: try XCTUnwrap(object))
        return try JSONDecoder().decode(KaraokeStyle.self, from: data)
    }

    func testPresetsMatchWindows() throws {
        let fixture = try shared()
        let presets = try XCTUnwrap(fixture["presets"] as? [String: Any])
        XCTAssertEqual(Set(presets.keys), Set(KaraokeStyle.Preset.allCases.map(\.rawValue)))
        for id in KaraokeStyle.Preset.allCases {
            XCTAssertEqual(KaraokeStyle.presets[id], try style(presets[id.rawValue]), id.rawValue)
        }
        XCTAssertEqual(KaraokeStyle.standard, KaraokeStyle.presets[KaraokeStyle.Preset(rawValue: fixture["defaultPreset"] as? String ?? "")!])
    }

    func testFieldNamesMatchWindows() throws {
        let presets = try XCTUnwrap(try shared()["presets"] as? [String: [String: Any]])
        let encoded = try JSONSerialization.jsonObject(with: JSONEncoder().encode(KaraokeStyle.standard)) as? [String: Any]
        XCTAssertEqual(Set(try XCTUnwrap(encoded).keys), Set(try XCTUnwrap(presets["classic"]).keys))
    }

    func testNormalizeMatchesWindows() throws {
        let fixture = try shared()
        let presets = try XCTUnwrap(fixture["presets"] as? [String: Any])
        let cases = try XCTUnwrap(fixture["normalize"] as? [[String: Any]])
        for (i, c) in cases.enumerated() {
            let expected = (c["output"] as? String).map { presets[$0] } ?? c["output"]
            XCTAssertEqual(KaraokeStyle.normalized(c["input"]), try style(expected), "case \(i)")
        }
        XCTAssertEqual(KaraokeStyle.normalized(nil), .standard)
        XCTAssertEqual(KaraokeStyle.decode(Data("garbage".utf8)), .standard)
    }

    func testPresetKeepsFontAndSize() {
        var mine = KaraokeStyle.standard
        mine.fontFamily = "Hiragino Sans"
        mine.fontSize = 72
        let pink = mine.applying(.pink)
        XCTAssertEqual(pink.fontFamily, "Hiragino Sans")
        XCTAssertEqual(pink.fontSize, 72)
        XCTAssertEqual(pink.matchingPreset, .pink)
        var edited = pink
        edited.sungFill = "#000001"
        XCTAssertNil(edited.matchingPreset)
    }

    func testHexRoundTrip() {
        let c = KaraokeStyle.rgb("#ec4f95")
        XCTAssertEqual(KaraokeStyle.hex(r: c.r, g: c.g, b: c.b), "#ec4f95")
    }
}
