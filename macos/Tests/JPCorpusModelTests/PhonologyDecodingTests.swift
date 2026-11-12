import Foundation
import JPCore
import XCTest

final class PhonologyDecodingTests: XCTestCase {
    // 字段与 jp-corpus/src/phonology.rs 的 serde 输出一致。
    private let provenanceJSON = #"{"snapshot":{"schemaVersion":41,"songs":3,"lines":12,"tokens":60,"corrections":0,"fingerprint":"fixture-lyrics-v1"},"rules":"analysis-v1","scope":"整个库","unit":"tokens","songs":3,"tokens":60}"#

    private func decode<T: Decodable>(_ type: T.Type, _ json: String) throws -> T {
        try JSONDecoder().decode(type, from: Data(json.utf8))
    }

    func testVowelResultWithUnivocalicGroups() throws {
        let result = try decode(VowelResult.self, """
        {"vowelMorae":40,"vowels":[{"vowel":"a","morae":12,"share":0.3},{"vowel":"o","morae":8,"share":0.2}],
         "sokuon":2,"hatsuon":3,"long":4,"words":20,"skipped":1,"skippedExamples":[["love",1]],
         "univocalic":[{"vowel":"a","distinctWords":1,"tokens":2,
           "words":[{"kana":"アナタ","lemma":"貴方","surfaces":[["あなた",2]],"morae":3,"freq":2,"songs":1}],
           "lineCount":1,"lines":[{"songId":"song-a","utteranceId":101,"text":"さらさら","kana":"サラサラ","morae":4}]}],
         "provenance":\(provenanceJSON)}
        """)
        XCTAssertEqual(result.vowelMorae, 40)
        XCTAssertEqual(result.vowels.map(\.vowel), [.a, .o])
        XCTAssertEqual(result.skippedExamples, [MorphologyExample(word: "love", count: 1)])
        let group = try XCTUnwrap(result.univocalic.first)
        XCTAssertEqual(group.vowel, .a)
        XCTAssertEqual(group.words.first?.surfaces.first?.word, "あなた")
        XCTAssertEqual(group.lines.first?.utteranceId, 101)
    }

    func testRhymeResultKeepsOptionalOffsets() throws {
        let result = try decode(RhymeResult.self, """
        {"pattern":"oo(ou)","lines":2,"songs":1,
         "hits":[{"songId":"song-a","title":"歌","artist":"誰か","utteranceId":7,"timeSec":12.5,"text":"東京へ","kana":"トーキョーエ","matched":"東京","charStart":0,"charEnd":2},
                 {"songId":"song-a","title":"歌","artist":"","utteranceId":8,"timeSec":null,"text":"思う","kana":"オモウ","matched":"思う","charStart":null,"charEnd":null}],
         "carriers":[{"text":"東京","kana":"トーキョー","lines":1,"songs":1}],
         "provenance":\(provenanceJSON)}
        """)
        XCTAssertEqual(result.pattern, "oo(ou)")
        XCTAssertEqual(result.hits.first?.charEnd, 2)
        XCTAssertNil(result.hits.last?.charStart)
        XCTAssertNil(result.hits.last?.timeSec)
        XCTAssertEqual(result.carriers.first?.kana, "トーキョー")
    }

    func testRhymeStatsWithMissingLift() throws {
        let result = try decode(RhymeStatsResult.self, """
        {"tail":3,"lengths":[{"length":1,"adjacent":10,"adjacentRhymed":4,"adjacentRate":0.4,"baseline":45,"baselineRhymed":9,"baselineRate":0.2,"lift":2.0},
                             {"length":2,"adjacent":10,"adjacentRhymed":0,"adjacentRate":0.0,"baseline":45,"baselineRhymed":0,"baselineRate":0.0,"lift":null}],
         "sameWord":1,
         "tails":[{"tail":"aiu","pairs":3,"songs":2,"sameWord":1,"example":{"songId":"song-a","first":"会いたい","second":"歌う"}}],
         "songs":[{"songId":"song-a","title":"歌","artist":"誰か","pairs":9,"rhymed":4,"rate":0.444}],
         "provenance":\(provenanceJSON)}
        """)
        XCTAssertEqual(result.lengths.first?.lift, 2.0)
        XCTAssertNil(result.lengths.last?.lift)
        XCTAssertEqual(result.tails.first?.example.second, "歌う")
        XCTAssertEqual(result.songs.first?.rhymed, 4)
    }

    func testQueriesEncodeTheFieldNamesRustReads() throws {
        let rhyme = try JSONSerialization.jsonObject(with: JSONEncoder().encode(RhymeQuery(pattern: "aiu", position: .start, strict: true))) as? [String: Any]
        XCTAssertEqual(rhyme?["pattern"] as? String, "aiu")
        XCTAssertEqual(rhyme?["position"] as? String, "start")
        XCTAssertEqual(rhyme?["strict"] as? Bool, true)
        let stats = try JSONSerialization.jsonObject(with: JSONEncoder().encode(RhymeStatsQuery(tail: 4))) as? [String: Any]
        XCTAssertEqual(stats?["tail"] as? Int, 4)
        XCTAssertEqual(stats?["position"] as? String, "end")
        let vowels = try JSONSerialization.jsonObject(with: JSONEncoder().encode(VowelQuery(minWordMorae: 3))) as? [String: Any]
        XCTAssertEqual(vowels?["minWordMorae"] as? Int, 3)
    }
}
