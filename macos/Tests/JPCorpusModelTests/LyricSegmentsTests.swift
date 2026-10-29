import JPCore
@testable import JPCorpusModel
import XCTest

/// 和 Windows 版 app/src/lyricSegments.ts 同一套规则：词按码点对齐原文、注音不跨词、对不上不猜
final class LyricSegmentsTests: XCTestCase {
    private func tokens(_ json: String) throws -> [LineToken] { try decoded([LineToken].self, json) }
    private func rubies(_ json: String) throws -> [Ruby] { try decoded([Ruby].self, json) }

    func testTokensAlignWithGapsAndRubiesStayInsideTheirWord() throws {
        let line = "人生、二十七で"
        let toks = try tokens(#"[{"surface":"人生","lemma":"人生","pos":"NOUN"},{"surface":"、","lemma":"、","pos":"PUNCT"},{"surface":"二十七","lemma":"二十七","pos":"NUM"},{"surface":"で","lemma":"で","pos":"ADP"}]"#)
        let ruby = try rubies(#"[{"start":0,"end":2,"reading":"じんせい"},{"start":3,"end":6,"reading":"にじゅうなな"}]"#)
        let segments = try XCTUnwrap(LyricSegments.segment(line, tokens: toks, rubies: ruby))
        XCTAssertEqual(segments, [
            .token(index: 0, pieces: [.init(text: "人生", reading: "じんせい")]),
            .token(index: 1, pieces: [.init(text: "、")]),
            .token(index: 2, pieces: [.init(text: "二十七", reading: "にじゅうなな")]),
            .token(index: 3, pieces: [.init(text: "で")]),
        ])
    }

    func testWhitespaceBetweenWordsBecomesGapAndKanjiInsideWordSplitsAroundRuby() throws {
        let line = "君も　考える"
        let toks = try tokens(#"[{"surface":"君","lemma":"君","pos":"PRON"},{"surface":"も","lemma":"も","pos":"ADP"},{"surface":"考える","lemma":"考える","pos":"VERB"}]"#)
        let ruby = try rubies(#"[{"start":0,"end":1,"reading":"きみ"},{"start":3,"end":4,"reading":"かんが"}]"#)
        let segments = try XCTUnwrap(LyricSegments.segment(line, tokens: toks, rubies: ruby))
        XCTAssertEqual(segments, [
            .token(index: 0, pieces: [.init(text: "君", reading: "きみ")]),
            .token(index: 1, pieces: [.init(text: "も")]),
            .gap("　"),
            .token(index: 2, pieces: [.init(text: "考", reading: "かんが"), .init(text: "える")]),
        ])
    }

    func testMisalignedTokensReturnNilAndCrossingRubyFallsBackToPlainClickableWords() throws {
        let toks = try tokens(#"[{"surface":"心臓","lemma":"心臓","pos":"NOUN"},{"surface":"が","lemma":"が","pos":"ADP"}]"#)
        XCTAssertNil(LyricSegments.segment("心が", tokens: toks, rubies: []), "分词和原文对不上：不猜")
        let crossing = try rubies(#"[{"start":1,"end":3,"reading":"ぞうが"}]"#)
        XCTAssertNil(LyricSegments.segment("心臓が", tokens: toks, rubies: crossing))
        let fallback = try XCTUnwrap(LyricSegments.clickable("心臓が", tokens: toks, rubies: crossing))
        XCTAssertFalse(fallback.rubiesShown, "注音对不上就不注音，词照样能点")
        XCTAssertEqual(fallback.segments.count, 2)
        XCTAssertNil(LyricSegments.clickable("心臓が", tokens: [], rubies: []), "没分词就没有能点的词")
    }

    func testCodePointsNotUTF16AndCombiningCharacters() throws {
        // 𠮷 是 UTF-16 代理对，位置按码点算（和 Rust 一样）
        let toks = try tokens(#"[{"surface":"𠮷野","lemma":"𠮷野","pos":"PROPN"},{"surface":"家","lemma":"家","pos":"NOUN"}]"#)
        let ruby = try rubies(#"[{"start":2,"end":3,"reading":"や"}]"#)
        let segments = try XCTUnwrap(LyricSegments.segment("𠮷野家", tokens: toks, rubies: ruby))
        XCTAssertEqual(segments.last, .token(index: 1, pieces: [.init(text: "家", reading: "や")]))
    }

    func testLookupTextAndCandidatesListEveryWordExceptPunctuation() throws {
        let toks = try tokens(#"[{"surface":"何","lemma":"何","pos":"PRON"},{"surface":"も","lemma":"も","pos":"ADP"},{"surface":"いら","lemma":"要る","pos":"VERB"},{"surface":"ない","lemma":"ない","pos":"AUX"},{"surface":"！","lemma":"！","pos":"PUNCT"},{"surface":" ","lemma":" ","pos":"X"}]"#)
        XCTAssertEqual(LyricSegments.lookupText(toks, from: 2), "いらない！ ")
        XCTAssertEqual(LyricSegments.lookupText(toks, from: 9), "")
        XCTAssertEqual(LyricSegments.lookupCandidates(toks), [0, 1, 2, 3], "代词、助词、助动词也能查；标点和空白不列")
    }
}
