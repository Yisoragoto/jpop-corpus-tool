import Foundation
import XCTest
@testable import JPCore

final class SystemBridgeTests: XCTestCase {
    func testDiagnosticsAndReleaseValidationOffline() throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent("system-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: directory) }
        let core = try CorpusCore(databaseURL: CorpusCore.writeFixture(in: directory))
        let report: String = try core.call("diagnosticsReport", ["appVersion": AnyEncodable("9.8.7"), "system": AnyEncodable("macOS 14.5")])
        XCTAssertTrue(report.contains("9.8.7")); XCTAssertTrue(report.contains("macOS 14.5")); XCTAssertTrue(report.contains("corpus.db"))
        let path = directory.appendingPathComponent("diagnostics.txt").path
        let saved: String = try core.call("diagnosticsSave", ["path": AnyEncodable(path)])
        XCTAssertEqual(saved, path); XCTAssertFalse(try String(contentsOfFile: path, encoding: .utf8).isEmpty)
        let prefs: UpdatePreferences = try core.call("updatePreferences", ["appVersion": AnyEncodable("9.8.7")])
        XCTAssertEqual(prefs.current, "9.8.7")
        XCTAssertThrowsError(try core.callJSON("updateDownload")); XCTAssertThrowsError(try core.callJSON("updateInstall"))
        XCTAssertThrowsError(try core.callJSON("updateReleasePage", argsJSON: #"{"url":"https://evil.test/releases"}"#))
    }
}
