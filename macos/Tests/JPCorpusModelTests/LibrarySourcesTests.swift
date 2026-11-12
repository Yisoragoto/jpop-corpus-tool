import Foundation
@testable import JPCore
@testable import JPCorpusModel
import XCTest

/// 来源文件夹（#60）的测试替身：回答写死，记下调了什么
private final class FakeSources: LibrarySourcesCalling, @unchecked Sendable {
    var sources: [LibrarySource] = []
    var removed: [RemovedSong] = []
    var suggestions = SourceSuggestions(folders: [], looseSongs: 0)
    var plan: SourcePlan?
    var applied = SourceApplied(refreshed: 0, restored: 0, relinked: 0, movedOut: 0, skipped: 0)
    var synced = LyricSyncReport(updated: [], unchanged: 0, failed: [])
    private(set) var calls: [String] = []
    private(set) var applyRequest: SourceApplyRequest?
    private(set) var syncedIds: [String] = []

    func librarySources() async throws -> [LibrarySource] { calls.append("sources"); return sources }
    func librarySourcesAdd(_ folders: [SourceFolderInput]) async throws -> [LibrarySource] {
        calls.append("add:" + folders.map { "\($0.path)|\($0.recursive)" }.joined(separator: ","))
        return sources
    }
    func librarySourceSetEnabled(_ id: Int64, enabled: Bool) async throws -> [LibrarySource] { calls.append("enable:\(enabled)"); return sources }
    func librarySourcePlan(_ id: Int64) async throws -> SourcePlan {
        calls.append("plan:\(id)")
        guard let plan else { throw FakeError.unexpected("plan") }
        return plan
    }
    func librarySourceApply(_ id: Int64, _ request: SourceApplyRequest) async throws -> SourceApplied {
        calls.append("apply:\(id)"); applyRequest = request; return applied
    }
    func libraryRemovedSongs() async throws -> [RemovedSong] { removed }
    func libraryRestoreSongs(_ ids: [String]) async throws -> Int { calls.append("restore:" + ids.joined(separator: ",")); return ids.count }
    func librarySourceSuggestions() async throws -> SourceSuggestions { calls.append("suggest"); return suggestions }
    func librarySourceRemove(_ id: Int64, mode: SourceRemoveMode) async throws -> SourceRemoved {
        calls.append("remove:\(id):\(mode.rawValue)")
        return SourceRemoved(movedOut: mode == .moveOut ? 3 : 0, deleted: mode == .delete ? 3 : 0, sources: [])
    }
    func librarySyncLyrics(_ songIds: [String]) async throws -> LyricSyncReport { syncedIds = songIds; return synced }
    func libraryDeleteSong(_ songId: String) async throws -> DeleteResult { throw FakeError.unexpected("libraryDeleteSong") }
}

@MainActor
final class LibrarySourcesTests: XCTestCase {
    private func file(_ id: String, _ path: String = "/m/a.flac") -> SourceSongFile {
        SourceSongFile(songId: id, title: "曲\(id)", artist: "歌手", path: path)
    }

    private func plan(nothingFound: Bool = false, changed: [SourceSongFile] = [], lyrics: [SourceSongFile] = [],
                      gone: [SourceSongFile] = [], unchanged: Int = 0) throws -> SourcePlan {
        let info = try decoded(LibrarySourceInfo.self, #"{"id":7,"rootPath":"/m","label":"m","recursive":true,"enabled":true,"createdAt":"2026-11-12 03:00:00","lastScannedAt":null,"lastScanError":null}"#)
        return SourcePlan(source: info, newFiles: [], newSongs: 0, changed: changed, returned: [], moved: [], gone: gone,
                          lyricsChanged: lyrics, unchanged: unchanged, nothingFound: nothingFound)
    }

    private func source(songs: Int = 2, removed: Int = 0, enabled: Bool = true, present: Bool = true, scanned: String? = nil) throws -> LibrarySource {
        let stamp = scanned.map { "\"\($0)\"" } ?? "null"
        return try decoded(LibrarySource.self, #"{"id":7,"rootPath":"/m","label":"m","recursive":true,"enabled":\#(enabled),"createdAt":"x","lastScannedAt":\#(stamp),"lastScanError":null,"songs":\#(songs),"removed":\#(removed),"present":\#(present)}"#)
    }

    func testReviewStartsAllOnButKeepsGoneOffWhenNothingWasFound() throws {
        let gone = [file("a"), file("b")]
        XCTAssertTrue(SourceReview(plan: try plan(gone: gone)).on.contains(.gone))
        let empty = SourceReview(plan: try plan(nothingFound: true, gone: gone))
        XCTAssertFalse(empty.on.contains(.gone), "一个音频都没扫到时别默认移出")
        XCTAssertEqual(empty.pending, 0)
        XCTAssertEqual(empty.request.moveOut, [])
    }

    func testRequestAndLyricIdsFollowTheTickedGroups() throws {
        var review = SourceReview(plan: try plan(
            changed: [file("a", "/m/a.flac"), file("", "/m/new.flac"), file("b", "/m/b.flac")],
            lyrics: [file("b", "/m/b.lrc"), file("c", "/m/c.lrc")],
            gone: [file("d")], unchanged: 5
        ))
        XCTAssertEqual(review.pending, 6)
        XCTAssertEqual(review.unchangedShown, 3, "歌词文件变了的也算在 unchanged 里")
        XCTAssertEqual(review.request.refresh, ["/m/a.flac", "/m/new.flac", "/m/b.flac"])
        XCTAssertEqual(review.lyricSongIds, ["a", "b", "c"], "认不出的跳过，同一首只同步一次")
        review.on.remove(.changed)
        XCTAssertEqual(review.request.refresh, [])
        XCTAssertEqual(review.lyricSongIds, ["b", "c"])
        review.on.remove(.lyrics)
        XCTAssertEqual(review.lyricSongIds, [])
        XCTAssertEqual(review.pending, 1)
    }

    func testApplyWritesSyncsLyricsAndReportsWhatChanged() async throws {
        let fake = FakeSources()
        fake.sources = [try source()]
        fake.plan = try plan(changed: [file("a")], lyrics: [file("c", "/m/c.lrc")], gone: [file("d")])
        fake.applied = SourceApplied(refreshed: 1, restored: 0, relinked: 0, movedOut: 1, skipped: 2)
        fake.synced = LyricSyncReport(updated: ["a"], unchanged: 1, failed: [LyricSyncFailure(songId: "c", error: "坏了")])
        let model = LibrarySourcesModel(client: fake)
        var changes = 0
        model.onChanged = { changes += 1 }
        await model.reload()
        await model.scan(model.sources[0])
        XCTAssertNotNil(model.review)
        await model.apply()
        XCTAssertEqual(fake.applyRequest?.refresh, ["/m/a.flac"])
        XCTAssertEqual(fake.applyRequest?.moveOut, ["d"])
        XCTAssertEqual(fake.syncedIds, ["a", "c"])
        XCTAssertEqual(model.note, "刷新 1 首，移出 1 首，1 首换上了新歌词；2 项在扫描之后又变了，没写，再扫一次看看")
        XCTAssertEqual(model.errorMessage, "1 首歌词没同步上：c 坏了")
        XCTAssertNil(model.review)
        XCTAssertEqual(changes, 1)
        XCTAssertNil(model.busy)
    }

    func testSuggestionsArePreTickedOnlyWhenThereAreNoSources() async throws {
        let fake = FakeSources()
        fake.suggestions = SourceSuggestions(folders: [SourceSuggestion(path: "/a", recursive: true, songs: 3),
                                                       SourceSuggestion(path: "/b", recursive: false, songs: 1)], looseSongs: 2)
        let model = LibrarySourcesModel(client: fake)
        await model.reload()
        XCTAssertEqual(model.pickedSuggestions, ["/a", "/b"])
        model.pickedSuggestions.remove("/a")
        await model.addSuggested()
        XCTAssertTrue(fake.calls.contains("add:/b|false"), "\(fake.calls)")
        fake.sources = [try source()]
        await model.reload()
        XCTAssertNil(model.suggestions, "有了来源就不再建议")
    }

    func testRestoreAllWhenNothingSelectedAndRemoveReportsTheMode() async throws {
        let fake = FakeSources()
        fake.sources = [try source()]
        fake.removed = try decoded([RemovedSong].self, #"[{"id":"x","title":"t","artist":"a","audioPath":"/m/x.flac","removedAt":"r","source":"/m","audioPresent":true},{"id":"y","title":"t","artist":"a","audioPath":"/m/y.flac","removedAt":"r","source":"","audioPresent":false}]"#)
        let model = LibrarySourcesModel(client: fake)
        await model.reload()
        await model.restore()
        XCTAssertTrue(fake.calls.contains("restore:x,y"), "\(fake.calls)")
        model.selectedRemoved = ["y"]
        await model.restore()
        XCTAssertTrue(fake.calls.contains("restore:y"))
        await model.remove(model.sources[0], mode: .moveOut)
        XCTAssertEqual(model.note, "文件夹已移除，3 首移出了语料库，可以在下面恢复")
        fake.sources = [try source()]
        await model.reload()
        await model.remove(model.sources[0], mode: .delete)
        XCTAssertEqual(model.note, "文件夹已移除，删掉了 3 首（音频文件没动）")
    }

    func testStatusLineMatchesWindows() throws {
        XCTAssertEqual(LibrarySourcesModel.status(try source(songs: 2, removed: 1)), "2 首 · 1 首已移出 · 含子文件夹 · 还没扫描过")
        XCTAssertEqual(LibrarySourcesModel.status(try source(present: false)), "2 首 · 含子文件夹 · 找不到这个文件夹（盘没插上？）")
        XCTAssertEqual(LibrarySourcesModel.status(try source(enabled: false)), "2 首 · 含子文件夹 · 已停用：启动时不检查")
        let utc = try XCTUnwrap(TimeZone(identifier: "UTC"))
        let shown = LibrarySourcesModel.scanned("2026-11-12 03:04:05", timeZone: utc)
        XCTAssertTrue(shown.hasPrefix("上次扫描 "), shown)
        XCTAssertFalse(shown.contains("2026-11-12 03:04:05"), "认得出的时间要换成本机格式：\(shown)")
        XCTAssertEqual(LibrarySourcesModel.scanned("昨天"), "上次扫描 昨天")
    }
}
