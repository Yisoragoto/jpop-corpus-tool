import Foundation
import JPCore
import XCTest
@testable import JPCorpusModel

extension FakeCorpus: AnkiCalling {
    func ankiStatus() async throws -> AnkiStatus { try lyricValue("ankiStatus") }
    func ankiWords(pos: [String], skipStudied: Bool, minCount: Int64, songIds: [String], limit: Int) async throws -> [WordCandidate] { try lyricValue("ankiWords", arguments: [pos.joined(separator: ","), String(skipStudied), String(minCount), songIds.joined(separator: ","), String(limit)]) }
    func ankiPreview(lemma: String, pos: String, maxExamples: Int, maxDicts: Int?) async throws -> AnkiCard { try lyricValue("ankiPreview", arguments: [lemma, pos, String(maxExamples), maxDicts.map(String.init) ?? "null"]) }
    func ankiExportStart(_ request: AnkiBatchRequest) async throws -> Int { try lyricValue("ankiExportStart", arguments: request.words.map(\.lemma) + [request.deck, String(request.maxExamples), request.maxDicts.map(String.init) ?? "null", request.dupMode.rawValue, request.dupScope.rawValue, String(request.clipAudio)]) }
    func ankiUpdateStart(_ request: AnkiBatchRequest) async throws -> Int { try lyricValue("ankiUpdateStart", arguments: request.words.map(\.lemma) + [request.deck, request.dupScope.rawValue]) }
    func ankiRefreshPreview(deck: String, scope: AnkiRefreshScope) async throws -> Int { try lyricValue("ankiRefreshPreview", arguments: [deck, scope.rawValue]) }
    func ankiRefreshStart(deck: String, scope: AnkiRefreshScope) async throws { let _: Bool = try lyricValue("ankiRefreshStart", arguments: [deck, scope.rawValue]) }
    func ankiCancel() async throws { let _: Bool = try lyricValue("ankiCancel") }
    func ankiIsRunning() async throws -> Bool { try lyricValue("ankiIsRunning") }
    func ankiProgress() async throws -> JobProgress<AnkiJobResult> { try lyricValue("ankiProgress") }
    func ankiMiningReport(now: String, deckContains: String?) async throws -> MiningReportResult { try lyricValue("ankiMiningReport", arguments: [now, deckContains ?? "null"]) }
}

final class AnkiModelTests: XCTestCase {
    private let status = #"{"connected":false,"version":0,"decks":[],"noteTypeReady":false,"collectionPath":"/tmp/collection.anki2","knownWords":8,"studiedWords":3,"message":"未连接"}"#
    private let words = #"[{"lemma":"夜","pos":"NOUN","count":4,"songCount":2,"inAnki":true,"studied":false},{"lemma":"空","pos":"NOUN","count":3,"songCount":1,"inAnki":false,"studied":false}]"#
    private let card = #"{"expression":"夜","reading":"よる","meaning":"<b>夜</b>","sentence":"<b>夜</b>に","sentenceAudio":"","source":"歌","jlpt":"N5","pitch":"","freq":"1","partOfSpeech":"名詞","lookupTerm":"夜","noExamples":false,"noDefinitions":false}"#
    @MainActor private func fixture() async -> (AnkiModel, FakeCorpus) {
        let fake = FakeCorpus()
        await fake.respondToLyrics("ankiStatus", json: status); await fake.respondToLyrics("ankiWords", json: words)
        await fake.respondToLyrics("ankiIsRunning", json: "false")
        let model = AnkiModel(); await model.load(fake); return (model, fake)
    }
    func testStartedProgressDefaultDeckToggleCountsAndPercent() throws {
        let p = AnkiLogic.startedProgress("export", total: 2)
        XCTAssertEqual(p.done, 0); XCTAssertEqual(p.total, 2); XCTAssertFalse(p.finished); XCTAssertFalse(p.cancelled)
        XCTAssertEqual([p.lemma, p.outcome, p.message, p.error], ["", "", "", ""]); XCTAssertEqual(p.newSentences, 0)
        XCTAssertEqual(AnkiLogic.percent(nil), 0); XCTAssertEqual(AnkiLogic.percent(AnkiLogic.startedProgress("refresh", total: 0)), 0)
        var one = p; one.done = 1; one.outcome = "added"
        XCTAssertEqual(AnkiLogic.percent(one), 50); XCTAssertEqual(AnkiLogic.counts([one, one]), ["added":2])
        XCTAssertEqual(AnkiLogic.defaultDeck(["Default", "日本語"]), "日本語")
        XCTAssertEqual(AnkiLogic.defaultDeck(["Default", "JAPANESE"]), "JAPANESE")
        XCTAssertEqual(AnkiLogic.defaultDeck(["Default"]), "Default"); XCTAssertEqual(AnkiLogic.defaultDeck([]), "")
        XCTAssertEqual(AnkiLogic.toggle(["夜"], lemma: "夜"), []); XCTAssertEqual(AnkiLogic.toggle([], lemma: "夜"), ["夜"])
    }
    func testLocalTimestampUsesLocalCalendarAndZeroPads() {
        let date = Date(timeIntervalSince1970: 0)
        XCTAssertEqual(AnkiLogic.localTimestamp(date, timeZone: TimeZone(secondsFromGMT: 8*3600)!), "1970-01-01 08:00")
        XCTAssertEqual(AnkiLogic.localTimestamp(date, timeZone: TimeZone(secondsFromGMT: -3600)!), "1969-12-31 23:00")
    }
    // mine.test.ts 原用例。
    func testArtistSubdeckTakesFirstPerformerAndNeverAddsALevel() {
        XCTAssertEqual(AnkiLogic.artistSubdeck("JPOP", artist: "ヨルシカ"), "JPOP::ヨルシカ")
        XCTAssertEqual(AnkiLogic.artistSubdeck("JPOP", artist: "ずっと真夜中でいいのに。/森カリオペ"), "JPOP::ずっと真夜中でいいのに。")
        XCTAssertEqual(AnkiLogic.artistSubdeck("日本語::歌詞", artist: " Mrs. GREEN APPLE "), "日本語::歌詞::Mrs. GREEN APPLE")
        XCTAssertEqual(AnkiLogic.artistSubdeck("JPOP", artist: "A::B:::C"), "JPOP::A:B:C")
        XCTAssertNil(AnkiLogic.artistSubdeck("JPOP", artist: " / feat")); XCTAssertNil(AnkiLogic.artistSubdeck("", artist: "ヨルシカ"))
    }
    @MainActor func testOfflineLoadKeepsWordsAndLearningState() async {
        let (model, fake) = await fixture()
        XCTAssertEqual(model.status?.knownWords, 8); XCTAssertEqual(model.status?.connected, false); XCTAssertEqual(model.words.count, 2); XCTAssertNil(model.error)
        let calls = await fake.calls
        XCTAssertTrue(calls.contains(.init(name: "ankiWords", arguments: ["NOUN,PROPN,VERB,ADJ,ADV", "true", "1", "", "300"])))
    }
    @MainActor func testSelectionUsesVisibleWordsAndPreviewPassesLimits() async {
        let (model, fake) = await fixture(); model.picked = ["夜", "已消失"]
        XCTAssertEqual(AnkiLogic.selected(model.words, picked: model.picked).map(\.lemma), ["夜"])
        await fake.respondToLyrics("ankiPreview", json: card)
        model.options.maxExamples = 8; model.options.maxDicts = 0
        await model.showPreview(model.words[0], client: fake)
        XCTAssertEqual(model.preview?.reading, "よる")
        let calls = await fake.calls; XCTAssertTrue(calls.contains(.init(name: "ankiPreview", arguments: ["夜", "NOUN", "8", "0"])))
    }
    @MainActor func testExportAndUpdateForwardOptionsAndEmptySelectionFails() async {
        let (model, fake) = await fixture(); await model.start(fake); XCTAssertEqual(model.error, "先勾几个词")
        model.error = nil; model.picked = ["夜"]; model.options.deck = "日本語"; model.options.dupMode = .skip; model.options.dupScope = .root; model.options.clipAudio = false
        await fake.respondToLyrics("ankiExportStart", json: "1"); await model.start(fake)
        XCTAssertTrue(model.running); XCTAssertFalse(model.busy); XCTAssertEqual(model.progress?.job, "export")
        await fake.respondToLyrics("ankiUpdateStart", json: "1"); await model.start(fake, update: true)
        XCTAssertEqual(model.progress?.job, "update")
        let calls = await fake.calls
        XCTAssertTrue(calls.contains(.init(name: "ankiExportStart", arguments: ["夜", "日本語", "2", "3", "skip", "root", "false"])))
        XCTAssertTrue(calls.contains(.init(name: "ankiUpdateStart", arguments: ["夜", "日本語", "root"])))
    }
    @MainActor func testRefreshCountInvalidatesAndCancelWaitsForTerminalProgress() async {
        let (model, fake) = await fixture(); model.options.deck = "D"
        await fake.respondToLyrics("ankiRefreshPreview", json: "5"); await model.countRefresh(fake); XCTAssertEqual(model.refreshCount, 5)
        model.refreshScope = .rootAndChildren; XCTAssertNil(model.refreshCount)
        await model.countRefresh(fake); model.options.deck = "E"; XCTAssertNil(model.refreshCount)
        await model.countRefresh(fake); await fake.respondToLyrics("ankiRefreshStart", json: "true"); await model.startRefresh(fake)
        XCTAssertEqual(model.progress?.total, 5); XCTAssertNil(model.refreshCount); XCTAssertTrue(model.running)
        await fake.respondToLyrics("ankiCancel", json: "true"); await model.cancel(fake); XCTAssertTrue(model.running)
        await fake.respondToLyrics("ankiProgress", json: #"{"running":false,"finished":true,"cancelled":true,"done":5,"total":5,"stage":"完成","error":null,"result":{"progress":{"job":"refresh","done":5,"total":5,"lemma":"","outcome":"","newSentences":0,"message":"","finished":true,"cancelled":true,"error":""},"log":[]}}"#)
        await model.poll(fake); XCTAssertFalse(model.running); XCTAssertEqual(model.progress?.cancelled, true)
    }
    @MainActor func testFailuresReleaseBusyFlagsAndReportCanRetry() async {
        let (model, fake) = await fixture(); model.picked = ["夜"]; await model.start(fake)
        XCTAssertFalse(model.busy); XCTAssertFalse(model.running); XCTAssertNotNil(model.error)
        await model.countRefresh(fake); XCTAssertFalse(model.counting)
        await model.generateReport(fake); XCTAssertFalse(model.reporting); XCTAssertNil(model.report)
        await fake.respondToLyrics("ankiMiningReport", json: #"{"path":"/tmp/report.html","collectionPath":"/tmp/collection.anki2","cards":1,"studied":1,"words":1,"artists":1,"songs":1,"decks":1,"noteTypes":["Lyrics"],"openError":"打不开浏览器"}"#)
        await model.generateReport(fake); XCTAssertEqual(model.report?.cards, 1); XCTAssertEqual(model.report?.openError, "打不开浏览器")
    }
    @MainActor func testPollPreservesPerWordLogAndReportsWholeJobFailure() async {
        let (model, fake) = await fixture()
        await fake.respondToLyrics("ankiProgress", json: #"{"running":false,"finished":true,"cancelled":false,"done":2,"total":2,"stage":"完成","error":null,"result":{"progress":{"job":"export","done":2,"total":2,"lemma":"","outcome":"","newSentences":0,"message":"","finished":true,"cancelled":false,"error":"Anki 已关闭"},"log":[{"job":"export","done":1,"total":2,"lemma":"夜","outcome":"added","newSentences":0,"message":"","finished":false,"cancelled":false,"error":""}]}}"#)
        await model.poll(fake); XCTAssertEqual(model.error, "Anki 已关闭"); XCTAssertEqual(model.log.first?.lemma, "夜"); XCTAssertEqual(model.words.count, 2)
    }
}
