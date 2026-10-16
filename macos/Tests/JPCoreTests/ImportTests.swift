import Foundation
import XCTest

@testable import JPCore

/// macOS 客户端自己建库：空目录 → 导入「音频 + .lrc」文件夹 → 补上分词 → KWIC 查得到。
/// 经真的 libjp_ffi.a 调 Rust 的 jp-import / jp-tokenizer，检查跨语言的数据形状和那条链路。
///
/// 分词那一半要一份 Sudachi 词典：CI 把它缓存在 `JP_SUDACHI_SOURCE` 指的目录里（见 .github/workflows/macos.yml）；
/// 本机没设就跳过那一半。
final class ImportTests: XCTestCase {
    private var dir: URL!

    override func setUpWithError() throws {
        dir = FileManager.default.temporaryDirectory.appendingPathComponent("jpimport-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    }

    override func tearDownWithError() throws {
        try? FileManager.default.removeItem(at: dir)
    }

    /// 一秒钟的静音 WAV（16 位单声道 8kHz）
    private func writeWav(_ url: URL) throws {
        let rate: UInt32 = 8_000
        let dataLength = rate * 2
        var data = Data()
        func le<T: FixedWidthInteger>(_ value: T) { withUnsafeBytes(of: value.littleEndian) { data.append(contentsOf: $0) } }
        data.append(contentsOf: Array("RIFF".utf8)); le(UInt32(36 + dataLength))
        data.append(contentsOf: Array("WAVEfmt ".utf8)); le(UInt32(16))
        le(UInt16(1)); le(UInt16(1)); le(rate); le(rate * 2); le(UInt16(2)); le(UInt16(16))
        data.append(contentsOf: Array("data".utf8)); le(dataLength)
        data.append(Data(count: Int(dataLength)))
        try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        try data.write(to: url)
    }

    /// 歌手文件夹里两首歌，各带同名 .lrc
    private func musicFolder() throws -> URL {
        let artist = dir.appendingPathComponent("music/テスト歌手", isDirectory: true)
        try writeWav(artist.appendingPathComponent("テスト歌手 - 夜の歌.wav"))
        try "[00:01.00]夜の風が吹いている\n[00:04.00]君を待っている\n"
            .write(to: artist.appendingPathComponent("テスト歌手 - 夜の歌.lrc"), atomically: true, encoding: .utf8)
        try writeWav(artist.appendingPathComponent("テスト歌手 - 朝の歌.wav"))
        try "[00:01.00]朝の風が眩しい\n"
            .write(to: artist.appendingPathComponent("テスト歌手 - 朝の歌.lrc"), atomically: true, encoding: .utf8)
        return artist.deletingLastPathComponent()
    }

    private func waitForImport(_ client: CorpusClient) async throws -> JobProgress<ImportResult> {
        for _ in 0..<600 {
            let progress = try await client.importProgress()
            if progress.finished { return progress }
            try await Task.sleep(for: .milliseconds(50))
        }
        XCTFail("导入一直没跑完")
        throw CancellationError()
    }

    func testAFreshLibraryIsBuiltFromAFolderOfAudioAndLrc() async throws {
        let music = try musicFolder()
        // 库目录和 corpus.db 都还不存在：打开就是建库
        let library = dir.appendingPathComponent("Application Support/JPOP Corpus Tool", isDirectory: true)
        let client = CorpusClient(core: try CorpusCore(databaseURL: library.appendingPathComponent("corpus.db")))
        XCTAssertTrue(FileManager.default.fileExists(atPath: library.appendingPathComponent("corpus.db").path))
        let empty = try await client.tracks()
        XCTAssertTrue(empty.isEmpty)

        // 扫描 → 计划（不写库）
        let plan = try await client.scanFolder(music.path)
        XCTAssertEqual(plan.summary.total, 2)
        XCTAssertEqual(plan.summary.new, 2)
        XCTAssertTrue(plan.items.allSatisfy { $0.hasLyrics && $0.action.kind == .new })
        XCTAssertTrue(plan.items.contains { $0.title == "夜の歌" && $0.artist == "テスト歌手" })
        let stillEmpty = try await client.tracks()
        XCTAssertTrue(stillEmpty.isEmpty, "扫描不写库")

        // 导入：后台跑，轮询进度
        try await client.runImport(tokenize: false)
        let done = try await waitForImport(client)
        XCTAssertNil(done.error)
        XCTAssertFalse(done.cancelled)
        XCTAssertEqual(done.done, 2)
        let result = try XCTUnwrap(done.result)
        XCTAssertEqual(result.imported, 2)
        XCTAssertEqual(result.lyricLines, 3)
        XCTAssertEqual(result.tracks.first?.outcome.kind, "imported")

        let tracks = try await client.tracks()
        XCTAssertEqual(tracks.count, 2)
        let night = try XCTUnwrap(tracks.first { $0.title == "夜の歌" })
        let lyrics = try await client.lyrics(night.id)
        XCTAssertEqual(lyrics.count, 2)

        // 再扫一遍：全在库里了
        let again = try await client.scanFolder(music.path)
        XCTAssertEqual(again.summary.new, 0)
        XCTAssertEqual(again.summary.alreadyImported, 2)

        let missing = try await client.untokenizedCount()
        XCTAssertEqual(missing, 2)
        let status = try await client.tokenizerStatus()
        XCTAssertTrue(status.bundledDir.hasSuffix("sudachi"))

        // 分词那一半：装上词典 → 顺手补分词 → KWIC 查得到
        guard let source = ProcessInfo.processInfo.environment["JP_SUDACHI_SOURCE"], !source.isEmpty else {
            throw XCTSkip("没有 Sudachi 词典（设 JP_SUDACHI_SOURCE），跳过分词那一半")
        }
        let ready = try await client.tokenizerInstall(from: source)
        XCTAssertEqual(ready.tokenized.songs, 2)
        XCTAssertEqual(ready.tokenizeError, "")
        let installed = try await client.tokenizerStatus()
        XCTAssertTrue(installed.ready)
        XCTAssertEqual(installed.source, "library")
        XCTAssertTrue(FileManager.default.fileExists(atPath: library.appendingPathComponent("sudachi/system.dic").path))

        let hits = try await client.kwic(KwicQuery(keywords: ["風"], field: .lemma))
        XCTAssertEqual(hits.count, 2)
        let hit = try XCTUnwrap(hits.first)
        XCTAssertGreaterThan(hit.endSec ?? 0, hit.timeSec ?? 0)
        let window = try await client.listenWindow(hit.utteranceId)
        XCTAssertEqual(window?.songId, hit.songId)

        // 有词典之后导入的直接带分词
        let more = dir.appendingPathComponent("more/別の歌手", isDirectory: true)
        try writeWav(more.appendingPathComponent("別の歌手 - 雨.wav"))
        try "[00:01.00]雨の夜に風が吹く\n".write(to: more.appendingPathComponent("別の歌手 - 雨.lrc"), atomically: true, encoding: .utf8)
        let morePlan = try await client.scanFolder(more.path)
        XCTAssertTrue(morePlan.tokenizerReady)
        let started = try await client.runImport()
        XCTAssertTrue(started.tokenize)
        let moreDone = try await waitForImport(client)
        XCTAssertGreaterThan(moreDone.result?.tokens ?? 0, 0)
        let allHits = try await client.kwic(KwicQuery(keywords: ["風"], field: .lemma))
        XCTAssertEqual(allHits.count, 3)
    }

    func testScanningFilesReportsWhatIsNotAudio() async throws {
        let music = try musicFolder()
        let client = CorpusClient(core: try CorpusCore(databaseURL: dir.appendingPathComponent("lib/corpus.db")))
        let lrc = music.appendingPathComponent("テスト歌手/テスト歌手 - 夜の歌.lrc")
        let wav = music.appendingPathComponent("テスト歌手/テスト歌手 - 夜の歌.wav")
        let plan = try await client.scanFiles([wav.path, lrc.path])
        XCTAssertEqual(plan.summary.total, 1)
        XCTAssertEqual(plan.ignored, ["テスト歌手 - 夜の歌.lrc"])

        do {
            try await client.runImport()
            let done = try await waitForImport(client)
            XCTAssertEqual(done.result?.imported, 1)
            // 同一批不能导两次
            try await client.runImport()
            XCTFail("应当报错")
        } catch let error as CoreError {
            XCTAssertTrue(error.localizedDescription.contains("没有待导入"))
        }
    }

    func testLookupWithoutDictionariesIsEmptyAndTokenizeSaysWhy() async throws {
        let db = try CorpusCore.writeFixture(in: dir.appendingPathComponent("fixture"))
        let client = CorpusClient(core: try CorpusCore(databaseURL: db))
        let dictionaries = try await client.dictionaries()
        XCTAssertTrue(dictionaries.isEmpty)
        let found = try await client.lookup("夜の風")
        XCTAssertTrue(found.entries.isEmpty)
        let idle = try await client.tokenizerProgress()
        XCTAssertFalse(idle.running)
        XCTAssertFalse(idle.finished)
        XCTAssertNil(idle.result)
    }
}
