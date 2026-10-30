import Foundation
import XCTest
@testable import JPCorpusModel

final class AppModelTests: XCTestCase {
    @MainActor
    func testExistingLibraryDoesNotCreateADatabaseAndSelectionRequiresConfirmation() async throws {
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("model-open-\(UUID())")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: folder) }
        let suite = "AppModelTests-\(UUID())"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }
        let recorder = OpenRecorder()
        let model = AppModel(defaults: defaults, openClient: { await recorder.open($0) })

        await model.openExisting(folder: folder)
        XCTAssertTrue(model.errorMessage?.contains("没有 corpus.db") == true)
        var opened = await recorder.urls
        XCTAssertTrue(opened.isEmpty)
        XCTAssertFalse(FileManager.default.fileExists(atPath: folder.appendingPathComponent("corpus.db").path))

        await model.selectLibrary(folder: folder)
        XCTAssertEqual(model.pendingCreationURL, folder)
        opened = await recorder.urls
        XCTAssertTrue(opened.isEmpty)
        model.cancelCreation()
        XCTAssertNil(model.pendingCreationURL)
        await model.selectLibrary(folder: folder)
        // SwiftUI may dismiss before invoking the button action; the captured URL must survive.
        let confirmedFolder = try XCTUnwrap(model.pendingCreationURL)
        model.cancelCreation()
        await model.confirmCreation(in: confirmedFolder)
        opened = await recorder.urls
        XCTAssertEqual(opened, [folder.appendingPathComponent("corpus.db")])
        XCTAssertEqual(model.section, .importer)
        XCTAssertEqual(defaults.string(forKey: "libraryFolder"), folder.path)
        XCTAssertNotNil(model.client)
    }

    @MainActor
    func testPickingCorpusDbOpensItsFolder() async throws {
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("model-pick-db-\(UUID())")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: folder) }
        let db = folder.appendingPathComponent("corpus.db")
        try Data().write(to: db)
        XCTAssertEqual(try AppModel.libraryFolder(forPicked: db).standardizedFileURL, folder.standardizedFileURL)
        XCTAssertEqual(try AppModel.libraryFolder(forPicked: folder), folder)
        let other = folder.appendingPathComponent("backup.db")
        try Data().write(to: other)
        XCTAssertEqual(try AppModel.libraryLocation(forPicked: other).database, "backup.db")
        let text = folder.appendingPathComponent("notes.txt")
        try Data().write(to: text)
        XCTAssertThrowsError(try AppModel.libraryFolder(forPicked: text))

        let suite = "AppModelTests-\(UUID())"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }
        let recorder = OpenRecorder()
        let model = AppModel(defaults: defaults, openClient: { await recorder.open($0) })
        await model.selectLibrary(picked: text)
        XCTAssertNotNil(model.errorMessage)
        XCTAssertNil(model.client)
        await model.selectLibrary(picked: db)
        XCTAssertNil(model.pendingCreationURL)
        XCTAssertNotNil(model.client)
        // 改了名的库也能直接打开，而且下次启动还开它
        await model.selectLibrary(picked: other)
        XCTAssertNil(model.errorMessage)
        XCTAssertEqual(model.databaseName, "backup.db")
        XCTAssertEqual(defaults.string(forKey: "libraryDatabase"), "backup.db")
        let opened = await recorder.urls
        XCTAssertEqual(opened.map(\.lastPathComponent), ["corpus.db", "backup.db"])
    }

    @MainActor
    func testExistingDatabaseOpensWithoutCreationPromptAndKeepsAudioRootsKey() async throws {
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("model-existing-\(UUID())")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: folder) }
        try Data().write(to: folder.appendingPathComponent("corpus.db"))
        let suite = "AppModelTests-\(UUID())"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }
        defaults.set(["/tmp/audio"], forKey: "audioRoots")
        let recorder = OpenRecorder()
        let model = AppModel(defaults: defaults, openClient: { await recorder.open($0) })
        await model.selectLibrary(folder: folder)
        XCTAssertNil(model.pendingCreationURL)
        XCTAssertNotNil(model.client)
        let calls = await recorder.corpus.calls
        XCTAssertEqual(calls.first, .init(name: "setAudioRoots", arguments: ["/tmp/audio"]))
        XCTAssertEqual(defaults.string(forKey: "libraryFolder"), folder.path)
        XCTAssertEqual(model.audioRoots, ["/tmp/audio"])
    }
}
