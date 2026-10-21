import Foundation
import XCTest
@testable import JPCorpusModel

@MainActor
final class AppModelTests: XCTestCase {
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
