import XCTest
import JPCore
@testable import JPCorpusModel

/// PersonProfile.releases.test.ts 全部 4 例。
final class PersonReleasesTests: XCTestCase {
    private func track(_ id: String,_ title: String,_ year: String = "2024") throws -> Track { var t = try VisualFixture.work(id,1,nil).track; t.title = title; t.year = year; return t }
    private func group(_ key: String,_ title: String,_ tracks: [Track],_ year: String = "2024") -> AlbumGroup {
        AlbumGroup(key: key,albumId: key.hasPrefix("id:") ? Int64(key.dropFirst(3)) : nil,title: title,year: year,tracks: tracks)
    }
    func testGuessReleaseTypeAndNewestFirst() throws {
        let groups = try [group("id:1","二人称",[track("a","1"),track("b","2"),track("c","3"),track("d","4")]),group("id:2","修羅 - Single",[track("e","修羅")],"2025"),group("id:3","あぶく",[track("f","あぶく")],"2026"),group("id:4","創作 - EP",[track("g","春泥棒"),track("h","強盗と花束")],"2021"),group("id:5","盗作",[track("i","春ひさぎ")],"2020"),group("single","单曲",[track("j","老人と海","2021"),track("k","ただ君に晴れ","2018")])]
        let result = PersonReleases.splitReleases(groups)
        XCTAssertEqual(result.singles.map(\.title),["あぶく","修羅 - Single","創作 - EP","老人と海","ただ君に晴れ"]); XCTAssertEqual(result.singles.map(\.kind),["single","single","ep","single","single"]); XCTAssertEqual(result.albums.map(\.title),["二人称","盗作"])
        XCTAssertEqual(result.singles.filter { $0.key == "single" }.map { $0.tracks.count },[1,1])
    }
    func testScrapedTypeTakesPriority() throws {
        let b = group("id:7","新宝島",[try track("x","聴きたかったダンスミュージック、リキッドルームに")])
        XCTAssertEqual(PersonReleases.releaseKind(b),"album"); XCTAssertEqual(PersonReleases.releaseKind(b,releaseType: "Single"),"single"); XCTAssertEqual(PersonReleases.releaseKind(b,releaseType: "ep"),"ep")
        XCTAssertEqual(PersonReleases.releaseKind(group("id:8","あぶく",[try track("y","あぶく")]),releaseType: "Album"),"album"); XCTAssertEqual(PersonReleases.releaseKind(group("id:9","Best",[try track("z","a")]),releaseType: "compile"),"album")
        XCTAssertEqual(PersonReleases.splitReleases([b],types: [7:"Single"]).singles.map(\.title),["新宝島"])
    }
    func testStripSingleAndEPSuffix() { XCTAssertEqual(PersonReleases.releaseTitle("修羅 - Single"),"修羅"); XCTAssertEqual(PersonReleases.releaseTitle("創作 – EP"),"創作"); XCTAssertEqual(PersonReleases.releaseTitle("Single"),"Single") }
    func testAllSongsNewestFirstStableAndDeduplicated() throws {
        let shared = try track("b","2","")
        let groups = try [group("id:1","二人称",[track("a","1",""),shared]),group("single","单曲",[track("e","修羅","2025"),shared]),group("id:3","あぶく",[track("f","あぶく","")],"2026")]
        XCTAssertEqual(PersonReleases.allSongs(groups).map(\.id),["f","e","a","b"])
    }
    func testGroupingUsesAlbumIdOrNameAndMergesDuplicateCredits() throws {
        var a = try track("a","1","2024"), b = try track("b","2","2023")
        a.albumId = 7; a.album = "Album"; b.albumId = 7; b.album = "Album"; b.coverPath = "cover"
        let groups = PersonReleases.groupByAlbum([a,a,b]); XCTAssertEqual(groups.count,1); XCTAssertEqual(groups[0].tracks.count,2); XCTAssertEqual(groups[0].year,"2023"); XCTAssertEqual(groups[0].coverPath,"cover")
        a.albumId = nil; XCTAssertEqual(PersonReleases.albumKey(a),"t:Album"); a.album = ""; XCTAssertEqual(PersonReleases.albumKey(a),"single")
    }
}
