import XCTest
@testable import JPCore

final class L10nTests: XCTestCase {
    private func table() throws -> Data {
        try JSONSerialization.data(withJSONObject: [
            "exact": ["检索": "Search", "未知错误": "Unknown error", "作词": "Lyrics", "夜": "night"],
            "patterns": [
                ["{0} 首", "{0} songs"],
                ["打不开语料库：{0}", "Cannot open the corpus: {0}"],
                ["{0}：{1}", "{0}: {1}"],
                ["迁移 {0} 首，跳过 {1} 首", "Migrated {0} songs, skipped {1}"],
            ],
        ])
    }

    func testChineseIsLeftAlone() throws {
        let l10n = L10n(english: false, json: try table())
        XCTAssertEqual(l10n.translate("检索"), "检索")
    }

    func testExactAndTemplates() throws {
        let l10n = L10n(english: true, json: try table())
        XCTAssertEqual(l10n.translate("检索"), "Search")
        XCTAssertEqual(l10n.translate("12 首"), "12 songs")
        XCTAssertEqual(l10n.translate("迁移 3 首，跳过 1 首"), "Migrated 3 songs, skipped 1")
        // 更具体的模板先用；填进去的中文值再翻一次
        XCTAssertEqual(l10n.translate("打不开语料库：未知错误"), "Cannot open the corpus: Unknown error")
        XCTAssertEqual(l10n.translate("作词：ACAね"), "Lyrics: ACAね")
        XCTAssertEqual(l10n.translate("没有这句"), "没有这句")
        XCTAssertEqual(l10n.translate("JPOP Corpus"), "JPOP Corpus")
    }

    func testJapaneseLyricsAreNotTakenApart() throws {
        let l10n = L10n(english: true, json: try table())
        // 带假名：不套模板（不然「君：さよなら」会变成别的样子）
        XCTAssertEqual(l10n.translate("君の 首"), "君の 首")
        XCTAssertEqual(l10n.translate("夜：さよなら"), "夜：さよなら")
    }

    func testMultilineIsTranslatedLineByLine() throws {
        let l10n = L10n(english: true, json: try table())
        XCTAssertEqual(l10n.translate("检索\n12 首"), "Search\n12 songs")
    }
}
