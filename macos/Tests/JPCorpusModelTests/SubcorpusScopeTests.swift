import Foundation
import XCTest
import JPCore
@testable import JPCorpusModel

final class SubcorpusScopeTests: XCTestCase {
    private func fixture() -> StatsFilter {
        var f = StatsFilter(); f.performerIds = [3]; f.jpOnly = true; f.yearFrom = 2010; f.yearTo = 2019; f.genres = ["J-Pop"]; f.languages = ["ja"]; f.minCompleteness = 0.8; f.distinctLines = true; return f
    }
    func testWholeCorpusAndDescription() {
        XCTAssertTrue(SubcorpusScope.isWholeCorpus(StatsFilter())); XCTAssertEqual(SubcorpusScope.describeFilter(StatsFilter()),"整个库")
        XCTAssertEqual(SubcorpusScope.describeFilter(fixture(),performerNames: ["YOASOBI"]),"YOASOBI · 2010–2019 年 · J-Pop · 日文 · 歌词完整度 ≥ 80%（仅日文，副歌只算一遍）")
        XCTAssertFalse(SubcorpusScope.isWholeCorpus(fixture()))
    }
    func testOneSidedYearsAndUnknownNames() {
        var f = StatsFilter(); f.performerIds = [1,2]; XCTAssertEqual(SubcorpusScope.describeFilter(f),"2 位演唱者")
        f.performerIds = []; f.yearFrom = 2020; XCTAssertEqual(SubcorpusScope.describeFilter(f),"2020 年起")
        f.yearFrom = nil; f.yearTo = 1999; XCTAssertEqual(SubcorpusScope.describeFilter(f),"1999 年前")
        f.yearFrom = 2001; f.yearTo = 2001; XCTAssertEqual(SubcorpusScope.describeFilter(f),"2001 年")
    }
    func testMethodIsNotScope() { var f = StatsFilter(); f.jpOnly = true; XCTAssertTrue(SubcorpusScope.isWholeCorpus(f)); XCTAssertEqual(SubcorpusScope.describeFilter(f),"整个库（仅日文）") }
    func testNormalizationSortsOnlyPerformers() {
        var a = StatsFilter(), b = StatsFilter(); a.performerIds = [2,1]; b.performerIds = [1,2]; XCTAssertTrue(SubcorpusScope.sameFilter(a,b)); b.distinctLines = true; XCTAssertFalse(SubcorpusScope.sameFilter(a,b))
        XCTAssertEqual(SubcorpusScope.normalizeFilter(StatsFilter()),StatsFilter())
    }
    func testWithScopePreservesCallerMethodsAndOverridesPerformers() {
        let scope = ActiveScope(id: 1,name: "范围",filter: fixture()); var local = StatsFilter()
        let a = SubcorpusScope.withScope(local,scope); XCTAssertEqual(a.performerIds,[3]); XCTAssertEqual(a.yearFrom,2010); XCTAssertFalse(a.jpOnly); XCTAssertFalse(a.distinctLines)
        local.performerIds = [7]; XCTAssertEqual(SubcorpusScope.withScope(local,scope).performerIds,[7]); XCTAssertEqual(SubcorpusScope.withScope(local,nil),local)
    }
    func testMergedScopeKeepsAllFieldsAndLocalNonemptyWins() {
        var f = fixture(); f.albumIds = [5]; f.songIds = ["001"]; let scope = ActiveScope(id: 1,name: "范围",filter: f)
        var local = Subcorpus(performerIds: [7],yearFrom: 2022); local.languages = ["en"]
        let result = SubcorpusScope.mergeSubcorpus(local,scope)
        XCTAssertEqual(result.performerIds,[7]); XCTAssertEqual(result.yearFrom,2022); XCTAssertEqual(result.yearTo,2019); XCTAssertEqual(result.genres,["J-Pop"]); XCTAssertEqual(result.languages,["en"]); XCTAssertEqual(result.albumIds,[5]); XCTAssertEqual(result.songIds,["001"]); XCTAssertEqual(result.minCompleteness,0.8)
        XCTAssertEqual(SubcorpusScope.mergeSubcorpus(local,nil),local)
    }
    @MainActor func testActiveScopePersistsAndClears() {
        let suite = "ScopeTests-\(UUID().uuidString)", defaults = UserDefaults(suiteName: suite)!
        defer { defaults.removePersistentDomain(forName: suite) }
        let store = SubcorpusStore(defaults: defaults), scope = ActiveScope(id: 2,name: "范围",filter: fixture())
        store.active = scope; XCTAssertEqual(SubcorpusStore(defaults: defaults).active,scope)
        store.active = nil; XCTAssertNil(SubcorpusStore(defaults: defaults).active)
        defaults.set(Data("broken".utf8),forKey: "jpop.activeSubcorpus"); XCTAssertNil(SubcorpusStore(defaults: defaults).active)
    }
}
