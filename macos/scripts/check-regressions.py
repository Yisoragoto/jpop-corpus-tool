#!/usr/bin/env python3
"""Temporarily reintroduce each bug; require an XCTest assertion failure, then restore sources."""
from pathlib import Path
import argparse
import subprocess


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    root = Path(__file__).resolve().parents[1]
    model = root / "Sources/JPCorpusModel/ImportModel.swift"
    view = root / "Sources/JPCorpusApp/ImportView.swift"
    app = root / "Sources/JPCorpusModel/AppModel.swift"
    scan = "let result = try await client.scanFiles(paths)"
    probes = [
        ("single-file-routing", model, scan,
         "let result = paths.count == 1 ? try await client.scanFolder(paths[0]) : try await client.scanFiles(paths)",
         "ImportModelTests.testSingleFileUsesScanFiles", False),
        ("folder-and-mixed-routing", model, scan, "let result = try await client.scanFolder(paths[0])",
         "ImportModelTests.testFoldersMultipleFilesAndMixedSelectionUseScanFiles", False),
        ("scan-error", model,
         "plan = result\n            tokenize = result.tokenizerReady\n        } catch {\n            errorMessage = error.localizedDescription",
         "plan = result\n            tokenize = result.tokenizerReady\n        } catch {\n            _ = error",
         "ImportModelTests.testScanAndPickerErrorsAreVisibleAndMissingLibraryExplainsNextStep", False),
        ("missing-library", model, "guard let client else {\n                errorMessage = Self.noLibrary",
         "guard let client else {\n                // Reintroduced silent failure",
         "ImportModelTests.testScanAndPickerErrorsAreVisibleAndMissingLibraryExplainsNextStep", False),
        ("duplicate-importer", view,
         "        .fileImporter(isPresented: $pick.isPresented,",
         "        .fileImporter(isPresented: $pick.isPresented, allowedContentTypes: [.folder]) { _ in }\n        .fileImporter(isPresented: $pick.isPresented,",
         "SourceGuardTests.testOnePresentationPerView", True),
        ("swallowed-picker-error", view,
         "case .failure(let error):\n                model.errorMessage = error.localizedDescription",
         "case .failure:\n                break",
         "SourceGuardTests.testFileImportersHandleFailure", True),
        ("implicit-library-creation", app,
         '''        guard Self.hasDatabase(in: folder, named: database) else {
            errorMessage = "这个文件夹没有 \\(database)：请选择已有的语料库，或确认新建一个库。"
            return
        }
''', "", "AppModelTests.testExistingLibraryDoesNotCreateADatabaseAndSelectionRequiresConfirmation", False),
    ]
    for name, path, before, after, test, skip_build in probes:
        original = path.read_text(encoding="utf-8")
        if original.count(before) != 1:
            raise RuntimeError(f"{name}: expected one mutation site, found {original.count(before)}")
        try:
            path.write_text(original.replace(before, after, 1), encoding="utf-8")
            command = ["swift", "test"] + (["--skip-build"] if skip_build else []) + ["--filter", test]
            result = subprocess.run(command, cwd=root, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
            (args.output / f"{name}.log").write_text(result.stdout, encoding="utf-8")
            assertions = [line for line in result.stdout.splitlines() if "error:" in line and ("XCTAssert" in line or "failed -" in line)]
            if result.returncode == 0 or not assertions:
                print(result.stdout, flush=True)
                raise RuntimeError(f"{name}: must fail an XCTest assertion (not compilation, skip, or infrastructure)")
            print(f"[mutation] {name} expected_failure=pass test={test}", flush=True)
            for line in assertions[:4]:
                print(line, flush=True)
        finally:
            path.write_text(original, encoding="utf-8")
    # The source and debug binary must both be back to the passing implementation.
    subprocess.run(["swift", "test", "--filter", "JPCorpusModelTests"], cwd=root, check=True)


if __name__ == "__main__":
    main()
