import XCTest
import JPCore
@testable import JPCorpusModel

/// timeline.test.ts 8 例与 releaseAxis.test.ts 4 例。
final class TimelineTests: XCTestCase {
    private func directory() throws -> LibraryDirectory {
        LibraryIndex.buildDirectory(try [VisualFixture.entry(1,"きのこ帝国",performer: 1,type: "Group",ended: "2019-05-27"),VisualFixture.entry(2,"佐藤千亜妃",performer: 1),VisualFixture.entry(3,"AKG",performer: 1)],[try VisualFixture.membership(band: "きのこ帝国",member: "佐藤千亜妃",memberId: 2,left: "2019-05-27",joined: "2007")])
    }
    private func rel(_ person: Int64,_ date: String,_ title: String? = nil,album: Int64? = nil,tracks: Int64 = 1) -> TimelineRelease {
        TimelineRelease(personId: person,albumId: album,title: title ?? date,date: date,tracks: tracks,songId: title ?? date)
    }
    private var releases: [TimelineRelease] { [rel(3,"2003"),rel(1,"2012-01-01"),rel(1,"2016"),rel(2,"2010"),rel(2,"2020-03-01")] }
    func testFractionalYears() {
        XCTAssertEqual(ReleaseTimeline.yearValue("2014"),2014.5); XCTAssertEqual(ReleaseTimeline.yearValue("2014-01-01"),2014)
        XCTAssertEqual(ReleaseTimeline.yearValue("2014-07")!,2014.54,accuracy: 0.05); XCTAssertNil(ReleaseTimeline.yearValue("?"))
    }
    func testMemberImmediatelyUnderBand() throws {
        let t = ReleaseTimeline.buildTimeline(try directory(),releases)
        XCTAssertEqual(t.rows.map(\.name),["AKG","きのこ帝国","佐藤千亜妃"]); XCTAssertEqual(t.rows.map(\.under),[nil,nil,1])
    }
    func testEndAndSoloConnector() throws {
        let t = ReleaseTimeline.buildTimeline(try directory(),releases)
        XCTAssertEqual(t.rows[1].end,2019.4,accuracy: 0.05); XCTAssertEqual(t.connectors.count,1)
        XCTAssertEqual(t.connectors[0].toAt,2020.17,accuracy: 0.05); XCTAssertEqual([t.min,t.max],[2003,2021])
    }
    func testFilterByMemberKeepsBand() throws { XCTAssertEqual(ReleaseTimeline.buildTimeline(try directory(),releases,query: "佐藤").rows.map(\.name),["きのこ帝国","佐藤千亜妃"]) }
    func testCrowdedYearsExpand() {
        let a = ReleaseTimeline.warpYears([[2020.1,2020.2,2020.3].map { WarpItem(at: $0,size: 40) },[WarpItem(at: 2021.5,size: 40)]],min: 2020,max: 2022,perYear: 90)
        XCTAssertEqual([a.x(2020),a.x(2021),a.x(2022),a.width,a.x(2021.5)],[0,144,234,324,189])
    }
    func testWarpInverseAndAnchoredZoom() {
        let rows = [[2020.1,2020.2,2020.3].map { WarpItem(at: $0,size: 40) },[WarpItem(at: 2021.5,size: 40)]]
        let a = ReleaseTimeline.warpYears(rows,min: 2020,max: 2022,perYear: 90)
        for year in [2020.0,2020.5,2021.0,2021.5,2022.0] { XCTAssertEqual(a.at(a.x(year)),year,accuracy: 1e-9) }
        XCTAssertEqual(a.at(-10),2020)
        // 放大后光标（视口内 300 处，左边留 40）下面还是同一年
        let b = ReleaseTimeline.warpYears(rows,min: 2020,max: 2022,perYear: 200)
        let before = a.at(50+300-40), offset = TimelinePan.zoomedOffset(50,anchor: 300,inset: 40,old: a,new: b)
        XCTAssertEqual(b.at(offset+300-40),before,accuracy: 1e-9)
        XCTAssertEqual(TimelinePan.clamp(-5,content: 500,viewport: 300),0)
        XCTAssertEqual(TimelinePan.clamp(900,content: 500,viewport: 300),200)
        XCTAssertEqual(TimelinePan.clamp(50,content: 200,viewport: 300),0)
    }
    func testRowPacking() { XCTAssertEqual(ReleaseTimeline.packRow([PackItem(x: 10,size: 40),PackItem(x: 12,size: 40),PackItem(x: 200,size: 30),PackItem(x: 14,size: 20)]),[10,56,200,92]) }
    private func tree(_ id: Int64) throws -> [TreePart] {
        let rs = [rel(1,"2014-10-08","フェイク",album: 7,tracks: 2),rel(2,"2018-06-27","空から"),rel(2,"2020-11-25","PLANET",album: 9)]
        let works = try [Int64(1):[VisualFixture.work("東京",1,7),VisualFixture.work("クロノスタシス",1,7)],Int64(2):[VisualFixture.work("空から",2,nil),VisualFixture.work("カタワレ",2,9)]]
        return ReleaseTimeline.buildTree(id,try directory(),rs,works)
    }
    func testTreeContinuesWithLaterSoloOnly() throws {
        let parts = try tree(1); XCTAssertEqual(parts.map(\.name),["きのこ帝国","佐藤千亜妃"]); XCTAssertNil(parts[0].after); XCTAssertEqual(parts[1].after?.bandName,"きのこ帝国")
        XCTAssertEqual(parts.map { $0.items.map { $0.release.release.title } },[["フェイク"],["PLANET"]])
        XCTAssertEqual(parts[0].items[0].songs.map(\.id),["東京","クロノスタシス"]); XCTAssertEqual(parts[1].items[0].songs.map(\.id),["カタワレ"])
    }
    func testSoloTreeHasAllReleases() throws {
        let parts = try tree(2); XCTAssertEqual(parts.count,1); XCTAssertEqual(parts[0].items.map { $0.release.release.title },["空から","PLANET"]); XCTAssertEqual(parts[0].items[0].songs.map(\.id),["空から"])
    }
    private func shape(_ years: [Int],_ end: Int? = nil) -> [String] {
        ReleaseTimeline.buildAxisColumns(years,endYear: end,year: { $0 }).map { col in
            switch col { case let .gap(a,b): return "\(a)…\(b)"; case let .year(y,items,ended): return "\(y):\(items.count)\(ended ? "!" : "")" }
        }
    }
    func testAxisGroupsSameYear() { XCTAssertEqual(shape([2019,2019,2020]),["2019:2","2020:1"]) }
    func testAxisFoldsLongGaps() { XCTAssertEqual(shape([2017,2019,2024]),["2017:1","2018:0","2019:1","2020…2023","2024:1"]) }
    func testAxisKeepsEndYear() { XCTAssertEqual(shape([2016,2017],2020),["2016:1","2017:1","2018…2019","2020:0!"]); XCTAssertEqual(shape([2016,2018],2018),["2016:1","2017:0","2018:1!"]) }
    func testEmptyAxis() { XCTAssertEqual(shape([],2020),[]) }
}
