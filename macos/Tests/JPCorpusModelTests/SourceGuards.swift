import Foundation
import XCTest

/// A deliberately small lexical guard, not a Swift parser. Strings/comments do not count as code.
enum SourceGuard {
    static func codeOnly(_ source: String) -> String {
        let chars = Array(source)
        var out = chars
        var i = 0
        func matches(_ value: String, at index: Int) -> Bool {
            let needle = Array(value)
            return index + needle.count <= chars.count && Array(chars[index..<(index + needle.count)]) == needle
        }
        func blank(_ start: Int, _ end: Int) {
            for n in start..<end where chars[n] != "\n" { out[n] = " " }
        }
        while i < chars.count {
            let start = i
            if matches("//", at: i) {
                while i < chars.count && chars[i] != "\n" { i += 1 }
                blank(start, i)
            } else if matches("/*", at: i) {
                var depth = 1
                i += 2
                while i < chars.count && depth > 0 {
                    if matches("/*", at: i) { depth += 1; i += 2 }
                    else if matches("*/", at: i) { depth -= 1; i += 2 }
                    else { i += 1 }
                }
                blank(start, i)
            } else if chars[i] == "\"" {
                let delimiter = matches("\"\"\"", at: i) ? "\"\"\"" : "\""
                i += delimiter.count
                while i < chars.count {
                    if chars[i] == "\\" { i = min(chars.count, i + 2) }
                    else if matches(delimiter, at: i) { i += delimiter.count; break }
                    else { i += 1 }
                }
                blank(start, i)
            } else { i += 1 }
        }
        return String(out)
    }

    static func presentationViolations(_ source: String, file: String) -> [String] {
        let code = codeOnly(source)
        let ns = code as NSString
        let pattern = #"\bstruct\s+(\w+)\s*:\s*[^\{]*\bView\b[^\{]*\{"#
        let regex = try! NSRegularExpression(pattern: pattern)
        var issues: [String] = []
        for match in regex.matches(in: code, range: NSRange(location: 0, length: ns.length)) {
            let name = ns.substring(with: match.range(at: 1))
            let start = match.range.location + match.range.length
            var depth = 1
            var end = start
            while end < ns.length && depth > 0 {
                let char = ns.character(at: end)
                if char == 123 { depth += 1 }
                if char == 125 { depth -= 1 }
                end += 1
            }
            let body = ns.substring(with: NSRange(location: start, length: end - start))
            for modifier in ["fileImporter", "sheet", "alert", "fileExporter"] {
                let uses = try! NSRegularExpression(pattern: "\\.\(modifier)\\s*\\(")
                let count = uses.numberOfMatches(in: body, range: NSRange(location: 0, length: (body as NSString).length))
                if count > 1 { issues.append("\(file): \(name) .\(modifier) count=\(count) (maximum 1)") }
            }
        }
        return issues
    }

    static func failureViolations(_ source: String, file: String) -> [String] {
        let code = codeOnly(source)
        let importer = try! NSRegularExpression(pattern: #"\.fileImporter\s*\("#)
        guard importer.firstMatch(in: code, range: NSRange(location: 0, length: (code as NSString).length)) != nil else { return [] }
        let failure = try! NSRegularExpression(pattern: #"case\s+\.failure\s*\([^)]*\)\s*:\s*(?!break\b|return\b|\})\S"#)
        guard failure.firstMatch(in: code, range: NSRange(location: 0, length: (code as NSString).length)) == nil else { return [] }
        return ["\(file): fileImporter needs an explicit case .failure handler (not break/return)"]
    }
}

final class SourceGuardTests: XCTestCase {
    private var sources: URL {
        URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent()
            .deletingLastPathComponent().appendingPathComponent("Sources")
    }

    private func swiftFiles() throws -> [URL] {
        let enumerator = try XCTUnwrap(FileManager.default.enumerator(at: sources, includingPropertiesForKeys: nil))
        let files = enumerator.compactMap { $0 as? URL }.filter { $0.pathExtension == "swift" }
        XCTAssertTrue(files.contains { $0.lastPathComponent == "ImportView.swift" })
        XCTAssertTrue(files.contains { $0.lastPathComponent == "SettingsView.swift" })
        return files
    }

    func testOnePresentationPerView() throws {
        for file in try swiftFiles() {
            let source = try String(contentsOf: file, encoding: .utf8)
            for issue in SourceGuard.presentationViolations(source, file: file.lastPathComponent) { XCTFail(issue) }
        }
    }

    func testFileImportersHandleFailure() throws {
        for file in try swiftFiles() {
            let source = try String(contentsOf: file, encoding: .utf8)
            for issue in SourceGuard.failureViolations(source, file: file.lastPathComponent) { XCTFail(issue) }
        }
    }

    func testPresentationGuardRejectsDuplicatesAndIgnoresCommentsAndStrings() {
        for modifier in ["fileImporter", "sheet", "alert", "fileExporter"] {
            let bad = "struct Broken: View { var body: some View { Text(\"x\").\(modifier)(a).\(modifier)(b) } }"
            let issues = SourceGuard.presentationViolations(bad, file: "Broken.swift")
            XCTAssertEqual(issues.count, 1)
            XCTAssertTrue(issues.first?.contains("Broken .\(modifier) count=2") == true)
        }
        let good = #"""
        // struct Fake: View { .fileImporter(a).fileImporter(b) }
        struct First: View { var body: some View { Text("} .fileImporter(fake)").fileImporter(a) } }
        /* .fileImporter(b) { nested /* } */ } */
        struct Second: View { var body: some View { Text("x").fileImporter(b) } }
        """#
        XCTAssertTrue(SourceGuard.presentationViolations(good, file: "Good.swift").isEmpty)
    }

    func testFailureGuardRejectsSuccessOnlyOrIgnoredFailure() {
        let prefix = "struct Broken: View { var body: some View { Text(\"x\").fileImporter(a) { result in "
        let successOnly = prefix + "if case .success(let urls) = result { use(urls) } } } }"
        XCTAssertFalse(SourceGuard.failureViolations(successOnly, file: "Broken.swift").isEmpty)
        let swallowed = prefix + "switch result { case .failure(let error): break; case .success(let urls): use(urls) } } } }"
        XCTAssertFalse(SourceGuard.failureViolations(swallowed, file: "Broken.swift").isEmpty)
        let handled = prefix + "switch result { case .failure(let error): model.errorMessage = error.localizedDescription; case .success(let urls): use(urls) } } } }"
        XCTAssertTrue(SourceGuard.failureViolations(handled, file: "Good.swift").isEmpty)
        // A sheet's content closure has no Result; requiring .failure there would be a false positive.
        XCTAssertTrue(SourceGuard.failureViolations("struct Sheet: View { Text(\"x\").sheet(a) { Text(\"ok\") } }", file: "Sheet.swift").isEmpty)
    }
}
