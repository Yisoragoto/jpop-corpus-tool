import JPCore
@testable import JPCorpusModel
import XCTest

/// 改读音（#63）的候选词：和 Windows 一样只给带汉字或「々」的词，按写法去重，预填现在显示的读音
final class ReadingEditTests: XCTestCase {
    private func line(_ text: String, _ tokens: String) throws -> LyricLine {
        try decoded(LyricLine.self, #"{"utteranceId":7,"lineIdx":0,"timeSec":null,"text":"\#(text)","tokens":\#(tokens)}"#)
    }

    func testOnlyKanjiWordsOnceEachWithTheShownReading() throws {
        let l = try line("夜に見つめる夜、人々", #"[{"surface":"夜","lemma":"夜","pos":"NOUN"},{"surface":"に","lemma":"に","pos":"ADP"},{"surface":"見つめる","lemma":"見つめる","pos":"VERB"},{"surface":"夜","lemma":"夜","pos":"NOUN"},{"surface":"、","lemma":"、","pos":"PUNCT"},{"surface":"人々","lemma":"人々","pos":"NOUN"}]"#)
        let rubies = try decoded([Ruby].self, #"[{"start":0,"end":1,"reading":"よる"},{"start":2,"end":3,"reading":"み"},{"start":6,"end":7,"reading":"よる"}]"#)
        let edits = ReadingEdit.candidates(songId: "004", line: l, rubies: rubies)
        XCTAssertEqual(edits.map(\.surface), ["夜", "見つめる", "人々"], "假名和标点不给改；同一写法只出现一次")
        XCTAssertEqual(edits.map(\.shown), ["よる", "みつめる", ""], "注音段换成读音、假名照抄；没注音的预填空")
        XCTAssertTrue(edits.allSatisfy { $0.songId == "004" })
    }

    func testIterationMarkCountsAsKanji() {
        XCTAssertTrue(ReadingEdit.hasKanji("々"))
        XCTAssertTrue(ReadingEdit.hasKanji("見る"))
        XCTAssertFalse(ReadingEdit.hasKanji("ひかり"))
        XCTAssertFalse(ReadingEdit.hasKanji("Love"))
    }

    func testRuleDecodesFromTheFfiShape() throws {
        let rule = try decoded(ReadingRule.self, #"{"songId":"","surface":"夜","reading":"ナイト","wholeLibrary":true}"#)
        XCTAssertEqual(rule.reading, "ナイト")
        XCTAssertTrue(rule.wholeLibrary)
    }
}
