import Foundation
import XCTest
import JPCore
@testable import JPCorpusModel

/// Windows libraryIndex.test.ts 的全部 10 例及未单测的辅助函数。
final class LibraryIndexTests: XCTestCase {
    private func person(_ id: Int64, _ name: String, performer: Int = 0, composer: Int = 0, lyricist: Int = 0, arranger: Int = 0, songs: Int = 0, ended: String = "", type: String = "") throws -> DirectoryEntry {
        let object: [String: Any] = ["id": id, "name": name, "imagePath": "", "artistType": type, "ended": ended, "imageManual": false, "performer": performer, "composer": composer, "lyricist": lyricist, "arranger": arranger, "songs": songs, "firstYear": "", "lastYear": ""]
        let data = try JSONSerialization.data(withJSONObject: object)
        return try JSONDecoder().decode(DirectoryEntry.self, from: data)
    }
    private func fixture() throws -> LibraryDirectory {
        let people = try [person(1, "きのこ帝国", performer: 19, songs: 19, ended: "2019-05-27", type: "Group"), person(2, "佐藤千亜妃", performer: 6, composer: 25, lyricist: 25, songs: 25), person(3, "西村ヒロ", arranger: 4, songs: 4), person(4, "ヨルシカ", performer: 30, songs: 30), person(5, "n-buna", composer: 30, lyricist: 30, arranger: 30, songs: 30)]
        let members = try decoded([Membership].self, #"[{"bandId":1,"bandName":"きのこ帝国","memberId":2,"memberName":"佐藤千亜妃","part":"主唱","joined":"2007","leftAt":"2019-05-27","source":"musicbrainz","bandImage":"","memberImage":""},{"bandId":1,"bandName":"きのこ帝国","memberId":9,"memberName":"あーちゃん","part":"吉他","joined":"2007","leftAt":"2019","source":"musicbrainz","bandImage":"","memberImage":""},{"bandId":4,"bandName":"ヨルシカ","memberId":5,"memberName":"n-buna","part":"作词作曲","joined":"2017","leftAt":"","source":"musicbrainz","bandImage":"","memberImage":""}]"#)
        return LibraryIndex.buildDirectory(people, members)
    }
    func testOwnPerformerCreditsAtTopLevel() throws { XCTAssertEqual(try fixture().musicians.map { $0.entry.name }, ["ヨルシカ", "きのこ帝国", "佐藤千亜妃"]) }
    func testNestedMembersAndMissingDirectoryEntry() throws {
        let band = try XCTUnwrap(fixture().musicians.first { $0.entry.id == 1 })
        XCTAssertTrue(band.isBand)
        XCTAssertEqual(band.members.map { $0.membership.memberName }, ["あーちゃん", "佐藤千亜妃"])
        XCTAssertNil(band.members[0].entry)
    }
    func testMembersExcludedFromBehind() throws { XCTAssertEqual(try fixture().behind.map(\.name), ["西村ヒロ"]) }
    func testSubtitles() throws {
        let dir = try fixture(), solo = try XCTUnwrap(dir.musicians.first { $0.entry.id == 2 }), band = try XCTUnwrap(dir.musicians.first { $0.entry.id == 1 })
        XCTAssertFalse(solo.isBand)
        XCTAssertEqual(LibraryIndex.musicianSubtitle(solo), "个人 · 6 首 · 曾属 きのこ帝国")
        XCTAssertEqual(LibraryIndex.musicianSubtitle(band), "乐队 · 19 首 · 2019 解散")
        XCTAssertEqual(LibraryIndex.memberSubtitle(band.members[1]), "前成员 · 2007–2019")
    }
    func testMemberSearchBringsBand() throws {
        let hits = LibraryIndex.filterMusicians(try fixture().musicians, "n-bu")
        XCTAssertEqual(hits.map { $0.row.entry.name }, ["ヨルシカ"])
        XCTAssertEqual(hits.map(\.memberHit), [true])
    }
    func testBehindRoleFilter() throws {
        let dir = try fixture()
        XCTAssertEqual(LibraryIndex.filterBehind(dir.behind, .arranger, "").count, 1)
        XCTAssertEqual(LibraryIndex.filterBehind(dir.behind, .composer, "").count, 0)
    }
    func testYearsAndSpans() {
        XCTAssertEqual(LibraryIndex.yearOf("2019-05-27"), "2019")
        XCTAssertEqual(LibraryIndex.spanOf("2017", ""), "2017–至今")
        XCTAssertEqual(LibraryIndex.spanOf("", "?"), "?–?")
    }
    func testMemberDateShapes() {
        for ok in ["", "?", "2019", "2019-05", "2019-05-27"] { XCTAssertTrue(LibraryIndex.validDate(ok)) }
        for bad in ["19", "2019/05", "去年", "2019-5"] { XCTAssertFalse(LibraryIndex.validDate(bad)) }
    }
    private func work(_ id: String, _ performers: [Int64], _ roles: [String] = []) throws -> PersonWork {
        let object: [String: Any] = ["id": id, "title": id, "artist": "", "album": "", "albumId": NSNull(), "year": "", "genre": "", "audioPath": "", "coverPath": "", "durationSec": NSNull(), "lineCount": 0, "roles": roles, "performerIds": performers]
        let data = try JSONSerialization.data(withJSONObject: object)
        return try JSONDecoder().decode(PersonWork.self, from: data)
    }
    func testSoloThenCollapsedBandThenCredits() throws {
        let band = try XCTUnwrap(fixture().bandsOf[2])
        let sections = LibraryIndex.sectionWorks(2, try [work("k1", [1], ["lyricist"]), work("s1", [2], ["performer"]), work("x1", [99], ["composer"])], band)
        XCTAssertEqual(sections.map(\.title), ["个人作品", "在 きのこ帝国 时", "合作 / 幕后"])
        XCTAssertEqual(sections.map(\.collapsed), [false, true, false])
        XCTAssertEqual(sections.map { $0.works.map(\.id) }, [["s1"], ["k1"], ["x1"]])
        XCTAssertEqual(sections[1].note, "2007–2019 · 这些歌属于乐队，其中 1 首有署名")
    }
    func testOnlyBandSectionIsOpenWithoutTitle() throws {
        let sections = LibraryIndex.sectionWorks(2, try [work("k1", [1])], try XCTUnwrap(fixture().bandsOf[2]))
        XCTAssertEqual(sections.count, 1); XCTAssertFalse(sections[0].collapsed); XCTAssertEqual(sections[0].title, "")
    }
    func testHelpersAndCurrentMemberOrder() throws {
        XCTAssertEqual(LibraryIndex.albumOwner(" A / B "), "A")
        XCTAssertEqual(LibraryIndex.rolesOf(try person(7, "作家", performer: 1, composer: 3, lyricist: 3, arranger: 2)), ["composer", "lyricist", "arranger", "performer"])
        XCTAssertTrue(LibraryIndex.sameView(.song("001"), .song("002")))
        XCTAssertFalse(LibraryIndex.sameView(.person(1), .person(2)))
        let dir = try fixture()
        let current = try XCTUnwrap(dir.musicians.first { $0.entry.id == 4 }?.members.first)
        XCTAssertEqual(LibraryIndex.memberSubtitle(current), "作词作曲")
        XCTAssertEqual(LibraryIndex.filterMusicians(dir.musicians, "  ").map(\.memberHit), [false, false, false])
    }
}
