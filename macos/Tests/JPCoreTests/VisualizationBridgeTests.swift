import Foundation
import XCTest
@testable import JPCore

final class VisualizationBridgeTests: XCTestCase {
    private var dir: URL!
    private var client: CorpusClient!
    override func setUpWithError() throws {
        dir = FileManager.default.temporaryDirectory.appendingPathComponent("jp-visual-\(UUID().uuidString)")
        client = CorpusClient(core: try CorpusCore(databaseURL: CorpusCore.writeFixture(in: dir)))
    }
    override func tearDownWithError() throws { client = nil; try? FileManager.default.removeItem(at: dir) }
    func testGraphAndReleaseShapes() async throws {
        let entries = try await client.peopleDirectory(), edges = try await client.collaborationEdges(), releases = try await client.timelineReleases()
        XCTAssertFalse(entries.isEmpty); XCTAssertFalse(releases.isEmpty)
        for e in edges { XCTAssertTrue(entries.contains { $0.id == e.source }); XCTAssertTrue(entries.contains { $0.id == e.target }); XCTAssertGreaterThan(e.shared,0) }
        for r in releases { XCTAssertFalse(r.title.isEmpty); XCTAssertFalse(r.date.isEmpty); XCTAssertGreaterThan(r.tracks,0) }
    }
    func testCloudRowsFormsAndScope() async throws {
        let all = try await client.statsFrequency(filter: StatsFilter(),pos: "NOUN",limit: 400)
        XCTAssertFalse(all.isEmpty); XCTAssertTrue(all.allSatisfy { $0.pos == "NOUN" && $0.freq > 0 })
        let forms = try await client.lemmaForms(all.map(\.lemma)); XCTAssertTrue(forms.allSatisfy { !$0.lemma.isEmpty })
        var f = StatsFilter(); f.yearFrom = 2999
        let empty = try await client.statsFrequency(filter: f,pos: "NOUN"); XCTAssertTrue(empty.isEmpty)
    }
    func testSavedScopeRoundTripPreviewAndDelete() async throws {
        let facets = try await client.subcorpusFacets(); XCTAssertNotNil(facets.yearMin); XCTAssertFalse(facets.genres.isEmpty)
        var f = StatsFilter(); f.performerIds = [1]; f.jpOnly = true; f.yearFrom = 2010; f.yearTo = 2025; f.genres = ["J-Pop"]; f.languages = ["ja"]; f.albumIds = [1]; f.songIds = ["001"]; f.minCompleteness = 0.8; f.distinctLines = true
        let saved = try await client.subcorpusSave(name: "范围",filter: f); XCTAssertEqual(saved.filter,f)
        let preview = try await client.subcorpusPreview(f); XCTAssertEqual(preview.songCount,saved.songCount); XCTAssertEqual(preview.lineCount,saved.lineCount)
        let updated = try await client.subcorpusSave(name: "范围",filter: StatsFilter()); XCTAssertEqual(updated.id,saved.id)
        let list = try await client.subcorpusList(); XCTAssertEqual(list.count,1)
        try await client.subcorpusDelete(saved.id); let empty = try await client.subcorpusList(); XCTAssertTrue(empty.isEmpty)
    }
    func testReportAndYearStatisticsDecode() async throws {
        let report = try await client.statsReport(StatsFilter()), years = try await client.timeline()
        XCTAssertEqual(report.report.songCount,4); XCTAssertFalse(report.text.isEmpty); XCTAssertFalse(report.report.topWords.isEmpty); XCTAssertFalse(years.isEmpty)
        let overview = try await client.overview()
        XCTAssertGreaterThan(overview.performers,0); XCTAssertGreaterThanOrEqual(overview.composers,0)
        XCTAssertGreaterThanOrEqual(overview.lyricists,0); XCTAssertGreaterThanOrEqual(overview.arrangers,0)
    }
}
