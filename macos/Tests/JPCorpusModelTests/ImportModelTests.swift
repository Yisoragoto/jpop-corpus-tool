import Foundation
import XCTest
@testable import JPCorpusModel

final class ImportModelTests: XCTestCase {
    @MainActor
    func testSingleFileUsesScanFiles() async {
        let fake = FakeCorpus()
        let model = ImportModel()
        let file = URL(fileURLWithPath: "/tmp/Artist - Song.wav")
        await model.scan([file], client: fake)
        let calls = await fake.calls
        XCTAssertEqual(calls, [.init(name: "scanFiles", arguments: [file.path])])
        XCTAssertEqual(model.plan?.summary.total, 1)
        XCTAssertNil(model.errorMessage)
        XCTAssertFalse(model.scanning)
    }

    @MainActor
    func testFoldersMultipleFilesAndMixedSelectionUseScanFiles() async {
        let folder = URL(fileURLWithPath: "/tmp/music", isDirectory: true)
        let files = (1...3).map { URL(fileURLWithPath: "/tmp/song\($0).wav") }
        for urls in [[folder], files, [folder, files[0]]] {
            let fake = FakeCorpus()
            let model = ImportModel()
            await model.scan(urls, client: fake)
            let calls = await fake.calls
            XCTAssertEqual(calls, [.init(name: "scanFiles", arguments: urls.map(\.path))])
            XCTAssertNotNil(model.plan)
        }
    }

    @MainActor
    func testScanAndPickerErrorsAreVisibleAndMissingLibraryExplainsNextStep() async {
        let fake = FakeCorpus()
        await fake.failScanning()
        let model = ImportModel()
        let urls = [URL(fileURLWithPath: "/tmp/song.wav")]
        await model.scan(urls, client: fake)
        XCTAssertTrue(model.errorMessage?.contains("没有读取权限") == true)
        XCTAssertNil(model.plan)
        XCTAssertFalse(model.scanning)

        model.errorMessage = nil
        await model.handleSelection(.failure(FakeError.injected), kind: .files, client: fake)
        XCTAssertTrue(model.errorMessage?.contains("没有读取权限") == true)

        model.errorMessage = nil
        let suite = "ImportModelTests-\(UUID())"
        let defaults = UserDefaults(suiteName: suite)!
        defer { defaults.removePersistentDomain(forName: suite) }
        let app = AppModel(defaults: defaults)
        XCTAssertNil(app.client)
        await model.handleSelection(.success(urls), kind: .files, client: app.client)
        XCTAssertTrue(model.errorMessage?.contains("还没有打开语料库") == true)
        XCTAssertTrue(model.errorMessage?.contains("新建或选择") == true)
    }

    @MainActor
    func testDismissalKeepsIntentUntilCompletionAndPickerCanBeUsedAgain() async {
        var pick = FilePickState<ImportPickKind>()
        pick.present(.dictionary)
        pick.isPresented = false // SwiftUI can dismiss before calling the completion.
        let kind = pick.finish()
        XCTAssertEqual(kind, .dictionary)
        XCTAssertNil(pick.intent)
        XCTAssertFalse(pick.isPresented)
        let fake = FakeCorpus()
        let model = ImportModel()
        await model.handleSelection(.success([URL(fileURLWithPath: "/tmp/dictionary")]), kind: kind, client: fake)
        let calls = await fake.calls
        XCTAssertEqual(calls, [.init(name: "tokenizerInstall", arguments: ["/tmp/dictionary"])])
        pick.present(.files)
        XCTAssertEqual(pick.finish(), .files)
    }
}
