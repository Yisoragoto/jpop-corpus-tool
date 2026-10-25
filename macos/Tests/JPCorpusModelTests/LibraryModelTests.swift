import Foundation
import XCTest
import JPCore
@testable import JPCorpusModel

final class LibraryModelTests: XCTestCase {
    private let editReport = #"{"changed":["title"],"performers":0,"albumId":null,"correctionsRenamed":0,"albumsRemoved":0,"peopleRemoved":0,"csv":"updated"}"#
    private let deletion = #"{"title":"歌","artist":"人","audioPath":"x","coverPath":"","csv":"noFile","lyricLines":2,"tokens":4,"credits":1,"plays":3,"correctionsKept":1,"albumsRemoved":1,"peopleRemoved":1,"pitchCacheRemoved":0,"coverRemoved":false}"#
    private let correction = #"{"utteranceId":7,"songId":"001","artist":"人","title":"歌","text":"夜が明ける","tokens":[{"surface":"夜","lemma":"夜","pos":"NOUN"}],"original":[{"surface":"夜","lemma":"夜","pos":"NOUN"}],"corrected":true}"#
    @MainActor
    private func defaults() -> UserDefaults { UserDefaults(suiteName: "LibraryModelTests-\(UUID().uuidString)")! }

    @MainActor
    func testSaveUsesClientAndReportsCSVProblemAfterSuccessfulWrite() async throws {
        let fake = FakeCorpus(), model = LibraryModel(client: fake, defaults: defaults())
        await fake.respondToLyrics("libraryEditSong", json: editReport.replacingOccurrences(of: "updated", with: "清单只读"))
        let saved = await model.save("001", edit: SongEdit(title: "新歌", artist: "人"))
        XCTAssertTrue(saved); XCTAssertEqual(model.version, 1); XCTAssertEqual(model.errorMessage, "清单只读")
        XCTAssertEqual(model.note, "改了歌名")
        let calls = await fake.calls
        XCTAssertEqual(calls.map(\.name), ["libraryEditSong"])
        let input = try decoded(SongEdit.self, calls[0].arguments[1])
        XCTAssertEqual(input.title, "新歌")
    }
    @MainActor
    func testDeletionRequiresPendingConfirmationAndFailureCanRetry() async {
        let fake = FakeCorpus(), model = LibraryModel(client: fake, defaults: defaults())
        let unconfirmed = await model.deleteConfirmed()
        XCTAssertFalse(unconfirmed)
        model.pendingDeletion = "001"
        let failed = await model.deleteConfirmed()
        XCTAssertFalse(failed); XCTAssertEqual(model.pendingDeletion, "001"); XCTAssertNotNil(model.errorMessage); XCTAssertFalse(model.busy)
        await fake.respondToLyrics("libraryDeleteSong", json: deletion)
        let deleted = await model.deleteConfirmed()
        XCTAssertTrue(deleted); XCTAssertNil(model.pendingDeletion)
        XCTAssertTrue(model.note.contains("1 条分词校正留着"))
    }
    @MainActor
    func testMissingClientCannotReportSuccessfulSave() async {
        let model = LibraryModel(defaults: defaults())
        let saved = await model.save("001", edit: SongEdit(title: "歌", artist: "人"))
        XCTAssertFalse(saved); XCTAssertTrue(model.errorMessage?.contains("请先打开语料库") == true)
    }
    @MainActor
    func testRelinkSuggestionsPreserveManualChoicesAndPartialFailure() async throws {
        let fake = FakeCorpus(), model = LibraryModel(client: fake, defaults: defaults())
        await fake.respondToLyrics("libraryMissingAudio", json: #"[{"songId":"001","title":"一","artist":"人","audioPath":"old"},{"songId":"002","title":"二","artist":"人","audioPath":"old2"}]"#)
        await model.loadMissing(); model.picks["001"] = "manual.wav"
        await fake.respondToLyrics("librarySuggestRelinks", json: #"[{"songId":"001","path":"auto.wav","reason":"tags"},{"songId":"002","path":"bad.wav","reason":"tags"}]"#)
        await model.suggest("/audio")
        XCTAssertEqual(model.picks["001"], "manual.wav")
        let before = await fake.calls
        XCTAssertFalse(before.contains { $0.name == "libraryRelinkAudio" })
        await fake.respondToLyrics("libraryRelinkAudio", json: #"{"results":[{"songId":"001","error":"","durationSec":1},{"songId":"002","error":"不能解码","durationSec":null}],"csv":"updated"}"#)
        let applied = await model.applyRelinks()
        XCTAssertTrue(applied); XCTAssertEqual(model.missing.map(\.songId), ["002"])
        XCTAssertEqual(model.picks, ["002": "bad.wav"]); XCTAssertEqual(model.repairErrors, ["002": "不能解码"])
    }
    @MainActor
    func testCorrectionsCleanBeforeSavingAndRevertNeedsSecondClick() async throws {
        let fake = FakeCorpus(), model = LibraryModel(client: fake, defaults: defaults())
        await fake.respondToLyrics("tokenCorrection", json: correction)
        await model.openCorrection(7)
        XCTAssertFalse(model.correctionDirty)
        model.tokens = [TokenEdit(" 夜空 ", lemma: "", pos: ""), TokenEdit(" ")]
        await fake.respondToLyrics("saveTokenCorrection", json: "true")
        let saved = await model.saveCorrection()
        XCTAssertTrue(saved)
        var calls = await fake.calls
        let call = try XCTUnwrap(calls.first { $0.name == "saveTokenCorrection" })
        XCTAssertEqual(try decoded([TokenEdit].self, call.arguments[1]), [TokenEdit("夜空")])
        let first = await model.revertCorrection()
        XCTAssertFalse(first); XCTAssertTrue(model.confirmRevert)
        calls = await fake.calls
        XCTAssertFalse(calls.contains { $0.name == "revertTokenCorrection" })
        await fake.respondToLyrics("revertTokenCorrection", json: "true")
        let second = await model.revertCorrection()
        XCTAssertTrue(second)
        await model.openCorrection(7)
        XCTAssertFalse(model.confirmRevert); XCTAssertTrue(model.selected.isEmpty)
    }
    @MainActor
    func testMissingCorrectionAndJobFailuresAreVisible() async {
        let fake = FakeCorpus(), model = LibraryModel(client: fake, defaults: defaults())
        await fake.respondToLyrics("tokenCorrection", json: "null")
        await model.openCorrection(999)
        XCTAssertTrue(model.errorMessage?.contains("这一行已经不在库里了") == true)
        await fake.respondToLyrics("libraryProgress", json: #"{"running":false,"finished":true,"cancelled":false,"done":0,"total":0,"stage":"失败","result":null,"error":"磁盘满了"}"#)
        await model.pollJob(); await model.pollJob()
        XCTAssertEqual(model.version, 1); XCTAssertEqual(model.errorMessage, "磁盘满了")
    }
    @MainActor
    func testIgnoresPersistAndDiscoveryUsesThem() async throws {
        let fake = FakeCorpus(), store = defaults()
        let model = LibraryModel(client: fake, defaults: store)
        model.ignoreFiles(["/audio/a.wav"])
        await fake.respondToLyrics("libraryNewAudio", json: #"{"folders":[],"scan":{"summary":{"total":0,"new":0,"alreadyImported":0,"possibleDuplicates":0,"duplicatesInBatch":0,"skipped":0},"items":[],"tokenizerReady":false,"ignored":[]}}"#)
        await model.ignoreFolder("/audio/old")
        XCTAssertEqual(LibraryModel(defaults: store).ignores, model.ignores)
        let calls = await fake.calls
        let arguments = try XCTUnwrap(calls.last?.arguments)
        XCTAssertEqual(try decoded([String].self, arguments[0]), ["/audio/old"])
        XCTAssertEqual(try decoded([String].self, arguments[1]), ["/audio/a.wav"])
        await model.restoreFolder("/audio/old")
        XCTAssertTrue(model.ignores.folders.isEmpty); XCTAssertEqual(model.ignores.files, ["/audio/a.wav"])
    }
    @MainActor
    func testLibraryAdminDescriptions() throws {
        XCTAssertNil(LibraryAdmin.csvProblem(nil)); XCTAssertNil(LibraryAdmin.csvProblem("unchanged"))
        XCTAssertEqual(LibraryAdmin.describeEdit(try decoded(EditReport.self, editReport)), "改了歌名，songs.csv 已同步")
        XCTAssertEqual(LibraryAdmin.describeDelete(try decoded(DeleteResult.self, deletion)), "已删除《歌》：2 行歌词、4 个词、3 条收听记录、1 张空专辑、1 个没有作品了的人；1 条分词校正留着，重新导入时恢复。音频文件没有动。")
    }
    @MainActor
    func testMigrationPreviewDoesNotWriteUntilConfirmedAndForwardsOptions() async throws {
        let fake = FakeCorpus(), model = LibraryModel(client: fake, defaults: defaults())
        let premature = await model.migrateConfirmed()
        XCTAssertFalse(premature)
        await fake.respondToLyrics("migratePlan", json: #"{"sourceSongs":1,"newSongs":1,"duplicateSongs":0,"lyricLines":0,"tokens":0,"people":1,"albums":0,"credits":1,"plays":0,"favorites":0,"corrections":0,"dictTerms":0,"jlptRows":0,"targetHasDictionaries":false,"samples":["人 - 歌"]}"#)
        await model.previewMigration("/source")
        var calls = await fake.calls
        XCTAssertEqual(calls.map(\.name), ["migratePlan"])
        XCTAssertEqual(model.migrationPlan?.newSongs, 1)
        model.migrationOptions.history = false
        await fake.respondToLyrics("migrateRun", json: "true")
        await fake.respondToLyrics("libraryProgress", json: #"{"running":true,"finished":false,"cancelled":false,"done":0,"total":0,"stage":"复制文件","result":null,"error":null}"#)
        let started = await model.migrateConfirmed()
        XCTAssertTrue(started); XCTAssertEqual(model.job?.running, true)
        calls = await fake.calls
        let arguments = try XCTUnwrap(calls.first { $0.name == "migrateRun" }?.arguments)
        XCTAssertEqual(try decoded(String.self, arguments[0]), "/source")
        let options = try decoded(MigrateOptions.self, arguments[1])
        XCTAssertFalse(options.history); XCTAssertTrue(options.dictionaries)
    }
    @MainActor
    func testMaintenanceRoutesAndSuccessfulJobOnlyBumpsVersionOnce() async {
        let fake = FakeCorpus(), model = LibraryModel(client: fake, defaults: defaults())
        for method in ["backfillDurations", "fillAlbumArtwork"] { await fake.respondToLyrics(method, json: "true") }
        await fake.respondToLyrics("libraryProgress", json: #"{"running":false,"finished":true,"cancelled":false,"done":1,"total":1,"stage":"完成","result":2,"error":null}"#)
        let artwork = await model.fillAlbumArtwork()
        XCTAssertTrue(artwork); XCTAssertEqual(model.note, "补齐 2 张专辑封面"); XCTAssertEqual(model.version, 1)
        await model.pollJob(); XCTAssertEqual(model.version, 1)
        await fake.respondToLyrics("libraryProgress", json: #"{"running":false,"finished":true,"cancelled":false,"done":1,"total":1,"stage":"完成","result":{"scanned":3,"written":1,"missing":1,"unknown":0,"failed":1,"samples":["坏文件"]},"error":null}"#)
        let durations = await model.backfillDurations()
        XCTAssertTrue(durations); XCTAssertTrue(model.note.contains("坏文件")); XCTAssertEqual(model.version, 2)
        let calls = await fake.calls
        XCTAssertTrue(calls.contains { $0.name == "backfillDurations" })
        XCTAssertTrue(calls.contains { $0.name == "fillAlbumArtwork" })
    }
}

final class MembersModelTests: XCTestCase {
    @MainActor
    private func prepare(_ fake: FakeCorpus) async {
        for method in ["bandMembers", "bandsOf", "membershipSuggestions"] { await fake.respondToLyrics(method, json: "[]") }
    }
    @MainActor
    func testInvalidDatesAndEmptyNameDoNotWrite() async {
        let fake = FakeCorpus(), model = MembersModel()
        let badDate = await model.put(.init(bandId: 1, memberName: "人", joined: "去年"), client: fake)
        let empty = await model.put(.init(bandId: 1, memberName: " "), client: fake)
        XCTAssertFalse(badDate); XCTAssertFalse(empty)
        let calls = await fake.calls
        XCTAssertTrue(calls.isEmpty)
    }
    @MainActor
    func testPutTrimsInputsReportsOutcomeAndReloads() async {
        let fake = FakeCorpus(), model = MembersModel()
        await prepare(fake); await fake.respondToLyrics("membershipPut", json: #""updated""#)
        let saved = await model.put(.init(bandId: 1, memberName: " 人 ", part: " 吉他 ", joined: " 2019 "), client: fake)
        XCTAssertTrue(saved); XCTAssertEqual(model.note, "改好了"); XCTAssertFalse(model.busy)
        let calls = await fake.calls
        XCTAssertEqual(calls.first?.arguments, ["1", "null", #""人""#, #""吉他""#, #""2019""#, #""""#])
        XCTAssertEqual(Set(calls.dropFirst().map(\.name)), Set(["bandMembers", "bandsOf", "membershipSuggestions"]))
    }
    @MainActor
    func testRemovalRequiresConfirmationAndSuggestionsUseDifferentCommands() async {
        let fake = FakeCorpus(), model = MembersModel()
        await prepare(fake)
        await model.removeConfirmed(1, client: fake)
        var calls = await fake.calls; XCTAssertTrue(calls.isEmpty)
        await fake.respondToLyrics("membershipReject", json: "true")
        model.removal = 2; await model.removeConfirmed(1, client: fake)
        XCTAssertNil(model.removal)
        await fake.respondToLyrics("membershipConfirm", json: #""keptExisting""#)
        await model.confirm(1, member: 3, client: fake)
        XCTAssertEqual(model.note, "已有更可靠的记录，没改")
        await model.rejectSuggestion(1, member: 4, client: fake)
        calls = await fake.calls
        XCTAssertEqual(calls.filter { $0.name == "membershipReject" }.map(\.arguments), [["1", "2"], ["1", "4"]])
        XCTAssertEqual(calls.first { $0.name == "membershipConfirm" }?.arguments, ["1", "3"])
    }
    @MainActor
    func testLoadFiltersSuggestionsByBandAndExposesFailure() async {
        let fake = FakeCorpus(), model = MembersModel()
        await prepare(fake)
        await fake.respondToLyrics("membershipSuggestions", json: #"[{"bandId":1,"memberId":2,"shared":3,"written":3,"bandName":"团","memberName":"人"},{"bandId":9,"memberId":3,"shared":4,"written":4,"bandName":"别团","memberName":"别人"}]"#)
        await model.load(1, client: fake)
        XCTAssertEqual(model.suggestions.map(\.memberId), [2])
        let failed = await model.put(.init(bandId: 1, memberName: "人"), client: fake)
        XCTAssertFalse(failed); XCTAssertNotNil(model.errorMessage); XCTAssertFalse(model.busy)
    }
}
