import Foundation
import JPCore
import XCTest

final class AnalysisExtrasDecodingTests: XCTestCase {
    // 字段与 analysis.rs / morphology.rs / lexical.rs / explore.rs 的 serde 输出一致。
    private let scopeJSON = #"{"subcorpus":{"performerIds":[7],"yearFrom":2000,"yearTo":2024,"albumIds":[],"genres":[],"songIds":[],"languages":[],"minCompleteness":null},"unit":"distinctLines","jpOnly":true}"#
    private let provenanceJSON = #"{"snapshot":{"schemaVersion":41,"songs":3,"lines":12,"tokens":60,"corrections":0,"fingerprint":"fixture-lyrics-v1"},"rules":"analysis-v1","scope":"整个库","unit":"distinctLines","songs":3,"tokens":60}"#

    private func decode<T: Decodable>(_ type: T.Type, _ json: String) throws -> T {
        try JSONDecoder().decode(type, from: Data(json.utf8))
    }

    func testNgramResultAndDispersion() throws {
        let result = try decode(NgramResult.self, """
        {"n":3,"types":24,"items":[{
          "words":["君","の","声"],"surface":"君の声","pos":["PRON","ADP","NOUN"],
          "freq":6,"per10k":1000.0,"songs":3,"range":1.0,
          "dispersion":{"dp":0.15,"dpNorm":0.225,"juillandD":null,"arf":4.8},
          "exampleUtteranceId":101,"exampleSongId":"song-a"
        }],"provenance":\(provenanceJSON)}
        """)
        XCTAssertEqual(result.n, 3)
        XCTAssertEqual(result.types, 24)
        let item = try XCTUnwrap(result.items.first)
        XCTAssertEqual(item.words, ["君", "の", "声"])
        XCTAssertEqual(item.per10k, 1000)
        XCTAssertEqual(item.dispersion.dpNorm, 0.225, accuracy: 0.0001)
        XCTAssertNil(item.dispersion.juillandD)
        XCTAssertEqual(item.dispersion.arf, 4.8, accuracy: 0.0001)
        XCTAssertEqual(item.exampleUtteranceId, 101)
        XCTAssertEqual(item.exampleSongId, "song-a")
        XCTAssertEqual(result.provenance.unit, .distinctLines)
    }

    func testInflectionTuplesAndPendingCount() throws {
        let result = try decode(InflectionResult.self, """
        {"total":8,"unannotated":2,"forms":[{
          "cform":"連用形-促音便","coarse":"連用形","freq":6,"share":0.75,"songs":2,
          "lemmas":[["歌う",4],["走る",2]],"surfaces":[["歌っ",4],["走っ",2]]
        },{
          "cform":"命令形","coarse":"命令形","freq":2,"share":0.25,"songs":1,
          "lemmas":[["走る",2]],"surfaces":[["走れ",2]]
        }],"provenance":\(provenanceJSON)}
        """)
        XCTAssertEqual(result.total, 8)
        XCTAssertEqual(result.unannotated, 2)
        let form = try XCTUnwrap(result.forms.first)
        XCTAssertEqual(form.cform, "連用形-促音便")
        XCTAssertEqual(form.coarse, "連用形")
        XCTAssertEqual(form.lemmas, [MorphologyExample(word: "歌う", count: 4), MorphologyExample(word: "走る", count: 2)])
        XCTAssertEqual(form.surfaces.first?.word, "歌っ")
        let tuple = try XCTUnwrap(form.lemmas.first)
        let encoded = try JSONEncoder().encode(tuple)
        let array = try XCTUnwrap(JSONSerialization.jsonObject(with: encoded) as? [Any])
        XCTAssertEqual(array.count, 2)
        XCTAssertEqual(array[0] as? String, "歌う")
        XCTAssertEqual(array[1] as? Int, 4)
        XCTAssertThrowsError(try decode(MorphologyExample.self, #"["歌う"]"#))
        XCTAssertThrowsError(try decode(MorphologyExample.self, #"["歌う",4,2]"#))
    }

    func testOrthographyReadingMergeAndWritingKinds() throws {
        let result = try decode(OrthographyResult.self, """
        {"groups":[{
          "normalized":"君","reading":"キミ","pos":"PRON","freq":9,"mergedByReading":true,
          "forms":[
            {"lemma":"君","writing":"kanji","freq":4,"songs":2,"exampleUtteranceId":101,"exampleSongId":"song-a"},
            {"lemma":"きみ","writing":"hiragana","freq":3,"songs":2,"exampleUtteranceId":202,"exampleSongId":"song-b"},
            {"lemma":"キミ","writing":"katakana","freq":2,"songs":1,"exampleUtteranceId":303,"exampleSongId":"song-c"}
          ]
        }],"totalGroups":12,"unannotated":5,"provenance":\(provenanceJSON)}
        """)
        XCTAssertEqual(result.totalGroups, 12)
        XCTAssertEqual(result.unannotated, 5)
        let group = try XCTUnwrap(result.groups.first)
        XCTAssertTrue(group.mergedByReading)
        XCTAssertEqual(group.normalized, "君")
        XCTAssertEqual(group.reading, "キミ")
        XCTAssertEqual(group.forms.map(\.writing), [.kanji, .hiragana, .katakana])
        XCTAssertEqual(group.forms.last?.exampleSongId, "song-c")
        XCTAssertEqual(try decode(Writing.self, #""kana""#), .kana)
        XCTAssertEqual(try decode(Writing.self, #""other""#), .other)
    }

    func testCqlReturnsExistingKwicHitsWithNullableTiming() throws {
        // cql::search 经 search::finish 返回 KwicHit，新增的上下文数组由旧模型忽略。
        let hits = try decode([KwicHit].self, #"""
        [{"songId":"song-a","artist":"演唱者","title":"声","audioPath":"/music/a.flac",
          "utteranceId":101,"lineIdx":2,"timeSec":null,"text":"君の声が聞こえる",
          "left":"","keyword":"君の声","right":"が聞こえる","repeatCount":2,"corrected":false,
          "endSec":null,"endKind":null,"matchLen":3,"leftWords":[],"rightWords":["が","聞こえる"],
          "linePos":2,"songLines":10}]
        """#)
        let hit = try XCTUnwrap(hits.first)
        XCTAssertEqual(hit.matchLen, 3)
        XCTAssertEqual(hit.repeatCount, 2)
        XCTAssertEqual(hit.keyword, "君の声")
        XCTAssertNil(hit.timeSec)
        XCTAssertNil(hit.endSec)
        XCTAssertNil(hit.endKind)
    }

    func testBreakdownUnknownYearAndPerformer() throws {
        let result = try decode(BreakdownResult.self, #"""
        {"by":"year","rows":[
          {"key":"","label":"","hits":0,"hitSongs":0,"tokens":100,"songs":1,"per10k":0.0,"ratio":0.0},
          {"key":"2020","label":"2020","hits":6,"hitSongs":2,"tokens":500,"songs":2,"per10k":120.0,"ratio":1.2}
        ],"hits":6,"tokens":600,"songs":3,"per10k":100.0}
        """#)
        XCTAssertEqual(result.by, .year)
        XCTAssertEqual(result.rows.first?.key, "")
        XCTAssertEqual(result.rows.last?.hitSongs, 2)
        XCTAssertEqual(result.rows.last?.per10k, 120)
        XCTAssertEqual(result.rows.last?.ratio, 1.2)
        XCTAssertEqual(result.per10k, 100)
        let performer = try decode(BreakdownResult.self, #"""
        {"by":"performer","rows":[{"key":"7","label":"演唱者","hits":6,"hitSongs":2,
         "tokens":600,"songs":3,"per10k":100.0,"ratio":1.0}],"hits":6,"tokens":600,"songs":3,"per10k":100.0}
        """#)
        XCTAssertEqual(performer.by, .performer)
        XCTAssertEqual(performer.rows.first?.label, "演唱者")
    }

    func testNetworkUsesNodeIndicesForEdges() throws {
        let result = try decode(NetworkResult.self, """
        {"node":"君","nodeFreq":12,"nodes":[
          {"lemma":"声","pos":"NOUN","freq":8,"cofreq":5,"logDice":13.0,"score":13.0},
          {"lemma":"聞こえる","pos":"VERB","freq":6,"cofreq":3,"logDice":12.415,"score":12.415}
        ],"edges":[{"a":0,"b":1,"freq":3,"logDice":12.778}],"provenance":\(provenanceJSON)}
        """)
        XCTAssertEqual(result.node, "君")
        XCTAssertEqual(result.nodeFreq, 12)
        let edge = try XCTUnwrap(result.edges.first)
        XCTAssertEqual(result.nodes[edge.a].lemma, "声")
        XCTAssertEqual(result.nodes[edge.b].lemma, "聞こえる")
        XCTAssertEqual(result.nodes[0].cofreq, 5)
        XCTAssertEqual(edge.logDice, 12.778, accuracy: 0.0001)
    }

    func testReusedLexicalModelHandlesShortTexts() throws {
        let result = try decode(LexicalResult.self, """
        {"rows":[{"key":"","label":"","songs":1,"tokens":4,"types":3,"ttr":0.75,
          "mattr":null,"mtld":null,"yuleK":1250.0,"hapaxRatio":0.6666666666666666,"chars":10,
          "scripts":{"kanji":0.3,"hiragana":0.4,"katakana":0.1,"latin":0.2,"other":0.0}
        },{"key":"7","label":"演唱者","songs":3,"tokens":600,"types":150,"ttr":0.25,
          "mattr":0.72,"mtld":28.5,"yuleK":135.0,"hapaxRatio":0.4,"chars":1200,
          "scripts":{"kanji":0.3,"hiragana":0.4,"katakana":0.1,"latin":0.15,"other":0.05}
        }],"provenance":\(provenanceJSON)}
        """)
        XCTAssertNil(result.rows[0].mattr)
        XCTAssertNil(result.rows[0].mtld)
        XCTAssertEqual(result.rows[0].scripts.latin, 0.2)
        XCTAssertEqual(result.rows[1].mattr, 0.72)
        XCTAssertEqual(result.rows[1].mtld, 28.5)
        XCTAssertEqual(result.rows[1].hapaxRatio, 0.4)
    }

    func testQueryDecodingAndCamelCaseEncoding() throws {
        let ngram = try decode(NgramQuery.self, """
        {"scope":\(scopeJSON),"n":3,"field":"surface","minFreq":2,"minSongs":2,"contains":null,"limit":2000}
        """)
        XCTAssertEqual(ngram.field, .surface)
        XCTAssertNil(ngram.contains)
        XCTAssertEqual(ngram.scope.subcorpus.performerIds, [7])
        let inflection = try decode(InflectionQuery.self, """
        {"scope":\(scopeJSON),"pos":null,"examples":8}
        """)
        XCTAssertNil(inflection.pos)
        XCTAssertEqual(inflection.examples, 8)
        let orthography = try decode(OrthographyQuery.self, """
        {"scope":\(scopeJSON),"minFreq":3,"minFormFreq":1,"contentOnly":true,"limit":null}
        """)
        XCTAssertTrue(orthography.contentOnly)
        XCTAssertNil(orthography.limit)
        let cql = try decode(CqlQuery.self, #"""
        {"query":"[lemma=\"君\"] [pos=\"ADP\"]","subcorpus":{"performerIds":[],"yearFrom":null,"yearTo":null,
         "albumIds":[],"genres":[],"songIds":[],"languages":[],"minCompleteness":null},"dedup":true,"crossLine":false,"limit":null}
        """#)
        XCTAssertEqual(cql.query, #"[lemma="君"] [pos="ADP"]"#)
        XCTAssertTrue(cql.dedup)
        XCTAssertNil(cql.limit)
        let breakdown = BreakdownQuery(by: .performer, cql: cql)
        XCTAssertEqual(try JSONDecoder().decode(BreakdownQuery.self, from: JSONEncoder().encode(breakdown)), breakdown)
        var kwic = KwicQuery(keywords: ["君"])
        kwic.subcorpus = ngram.scope.subcorpus
        let keywordBreakdown = BreakdownQuery(by: .year, kwic: kwic)
        XCTAssertEqual(try JSONDecoder().decode(BreakdownQuery.self, from: JSONEncoder().encode(keywordBreakdown)), keywordBreakdown)
        let network = try decode(NetworkQuery.self, """
        {"collocation":{"scope":\(scopeJSON),"node":"君","field":"lemma","left":3,"right":3,
          "minFreq":2,"collocatePos":null,"contentOnly":true,"measure":"logDice","limit":20},
         "size":20,"minPair":2,"perNode":3}
        """)
        XCTAssertEqual(network.collocation.node, "君")
        XCTAssertEqual(network.perNode, 3)
        let encoded = try JSONEncoder().encode(network)
        let object = try XCTUnwrap(JSONSerialization.jsonObject(with: encoded) as? [String: Any])
        XCTAssertEqual(object["minPair"] as? Int, 2)
        XCTAssertNil(object["min_pair"])
        XCTAssertEqual(try JSONDecoder().decode(NetworkQuery.self, from: encoded), network)
    }

    func testEmptyResultsRemainDecodable() throws {
        let ngram = try decode(NgramResult.self, """
        {"n":3,"types":0,"items":[],"provenance":\(provenanceJSON)}
        """)
        let inflection = try decode(InflectionResult.self, """
        {"total":0,"unannotated":0,"forms":[],"provenance":\(provenanceJSON)}
        """)
        let orthography = try decode(OrthographyResult.self, """
        {"groups":[],"totalGroups":0,"unannotated":0,"provenance":\(provenanceJSON)}
        """)
        let network = try decode(NetworkResult.self, """
        {"node":"君","nodeFreq":0,"nodes":[],"edges":[],"provenance":\(provenanceJSON)}
        """)
        XCTAssertTrue(ngram.items.isEmpty)
        XCTAssertTrue(inflection.forms.isEmpty)
        XCTAssertTrue(orthography.groups.isEmpty)
        XCTAssertTrue(network.edges.isEmpty)
    }
}
