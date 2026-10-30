import Foundation
import JPCore
import XCTest

final class KwicExploreDecodingTests: XCTestCase {
    private var directory: URL!
    private var client: CorpusClient!

    override func setUpWithError() throws {
        directory = FileManager.default.temporaryDirectory.appendingPathComponent("jp-kwic-explore-\(UUID().uuidString)")
        let database = try CorpusCore.writeFixture(in: directory)
        client = CorpusClient(core: try CorpusCore(databaseURL: database))
    }

    override func tearDownWithError() throws {
        client = nil
        try? FileManager.default.removeItem(at: directory)
    }

    func testCqlValidationAndScopedHitsDecodeThroughFFI() async throws {
        let expression = #"[lemma="夜"]"#
        try await client.cqlCheck(expression)
        do {
            try await client.cqlCheck(#"[lemma="夜""#)
            XCTFail("未闭合的 CQL 应返回语法错误")
        } catch let error as CoreError {
            guard case .call(let method, let message) = error else {
                return XCTFail("应是 Rust 语法错误：\(error)")
            }
            XCTAssertEqual(method, "cqlCheck")
            XCTAssertFalse(message.isEmpty)
        }
        let query = CqlQuery(query: expression, subcorpus: Subcorpus(songIds: ["004"]),
                             dedup: true, crossLine: true, limit: 5000)
        let hits = try await client.cqlSearch(query)
        XCTAssertFalse(hits.isEmpty)
        XCTAssertTrue(hits.allSatisfy { $0.songId == "004" && $0.keyword == "夜" && $0.matchLen == 1 })
        XCTAssertTrue(hits.contains { $0.repeatCount > 1 })
        let encoded = try JSONEncoder().encode(query)
        XCTAssertEqual(try JSONDecoder().decode(CqlQuery.self, from: encoded), query)
    }

    func testBothBreakdownQueriesDecodeAndUseTheSameScope() async throws {
        let scope = Subcorpus(songIds: ["004"])
        var keyword = KwicQuery(keywords: ["夜"], field: .lemma)
        keyword.subcorpus = scope
        keyword.dedup = true
        let cql = CqlQuery(query: #"[lemma="夜"]"#, subcorpus: scope, dedup: true)
        let keywordResult = try await client.kwicBreakdown(BreakdownQuery(kwic: keyword))
        let cqlResult = try await client.kwicBreakdown(BreakdownQuery(cql: cql))
        XCTAssertEqual(keywordResult, cqlResult)
        XCTAssertEqual(cqlResult.by, .year)
        XCTAssertGreaterThan(cqlResult.hits, 0)
        XCTAssertEqual(cqlResult.rows.reduce(Int64(0)) { $0 + $1.hits }, cqlResult.hits)
        XCTAssertEqual(cqlResult.songs, 1)
        XCTAssertGreaterThan(cqlResult.per10k, 0)
        let performers = try await client.kwicBreakdown(BreakdownQuery(by: .performer, cql: cql))
        XCTAssertEqual(performers.by, .performer)
        XCTAssertTrue(performers.rows.contains { !$0.label.isEmpty && $0.hitSongs == 1 })
    }

    func testNetworkDecodesFrequencyStrengthAndEdgeIndicesThroughFFI() async throws {
        var collocation = CollocationQuery(node: "夜", field: .lemma,
                                          scope: AnalysisScope(unit: .distinctLines))
        collocation.minFreq = 1
        collocation.limit = 20
        let query = NetworkQuery(collocation: collocation, minPair: 1)
        let result = try await client.collocationNetwork(query)
        XCTAssertEqual(result.node, "夜")
        XCTAssertGreaterThan(result.nodeFreq, 0)
        XCTAssertFalse(result.nodes.isEmpty)
        XCTAssertTrue(result.nodes.allSatisfy { $0.freq > 0 && $0.cofreq > 0 && $0.score.isFinite && $0.logDice.isFinite })
        for edge in result.edges {
            XCTAssertTrue(result.nodes.indices.contains(edge.a))
            XCTAssertTrue(result.nodes.indices.contains(edge.b))
            XCTAssertGreaterThan(edge.freq, 0)
            XCTAssertTrue(edge.logDice.isFinite)
        }
        XCTAssertEqual(result.provenance.unit, .distinctLines)
        let emptyScope = AnalysisScope(subcorpus: Subcorpus(songIds: ["missing"]))
        let emptyQuery = NetworkQuery(collocation: CollocationQuery(node: "夜", scope: emptyScope))
        let empty = try await client.collocationNetwork(emptyQuery)
        XCTAssertTrue(empty.nodes.isEmpty)
        XCTAssertTrue(empty.edges.isEmpty)
    }
}
