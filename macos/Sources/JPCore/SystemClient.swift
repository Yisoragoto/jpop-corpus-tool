import Foundation

public enum UpdateChannel: String, Codable, CaseIterable, Sendable { case stable, debug }
public struct UpdatePreferences: Codable, Equatable, Sendable {
    public var channel: UpdateChannel
    public var current: String
    public var buildId: String
    public var skippedStable: String
    public var skippedDebug: String
}
public struct UpdateRelease: Codable, Equatable, Identifiable, Sendable {
    public var buildId: String?
    public var version: String
    public var tag: String
    public var name: String
    public var notes: String
    public var publishedAt: String
    public var url: String
    public var prerelease: Bool
    /// 能在程序里直接装的那个包（macOS 是 .dmg）。没有就只能去发布页
    public var installer: UpdateAsset?
    public var id: String { tag }
    public var identity: String { buildId ?? version }
}
public struct UpdateAsset: Codable, Equatable, Sendable {
    public var name: String
    public var size: Int64?
}
public struct UpdateStatus: Codable, Equatable, Sendable {
    public var channel: UpdateChannel
    public var buildId: String
    public var skipped: Bool
    public var message: String
    public var current: String
    public var latest: UpdateRelease?
    public var updateAvailable: Bool
}
public protocol UpdateCalling: Sendable {
    func updatePreferences() async throws -> UpdatePreferences
    func updateCheck() async throws -> UpdateStatus
    func updateSetChannel(_ channel: UpdateChannel) async throws -> UpdatePreferences
    func updateSkip(_ channel: UpdateChannel, id: String) async throws -> UpdatePreferences
    func updateChangelog() async throws -> [UpdateRelease]
    /// 后台下载刚才检查到的那一版（`id` 是 `UpdateRelease.identity`，用来核对）
    func updateDownloadStart(id: String) async throws
    func updateDownloadProgress() async throws -> JobProgress<String>
    /// 拉起替换脚本：等本进程退出后把 `appPath` 换成新版并重新打开。调用方随后退出
    func updateInstall(appPath: String) async throws
}
extension CorpusClient: UpdateCalling {
    private var packageVersion: String? { Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String }
    private var versionArgs: [String: AnyEncodable] { packageVersion.map { ["appVersion": AnyEncodable($0)] } ?? [:] }
    public func updatePreferences() async throws -> UpdatePreferences {
        let args = versionArgs
        return try await performDetached { try $0.call("updatePreferences", args) }
    }
    public func updateCheck() async throws -> UpdateStatus {
        let args = versionArgs
        return try await performDetached { try $0.call("updateCheck", args) }
    }
    public func updateSetChannel(_ channel: UpdateChannel) async throws -> UpdatePreferences {
        var built = versionArgs; built["channel"] = AnyEncodable(channel)
        let args = built
        return try await performDetached { try $0.call("updateSetChannel", args) }
    }
    public func updateSkip(_ channel: UpdateChannel, id: String) async throws -> UpdatePreferences {
        var built = versionArgs; built["channel"] = AnyEncodable(channel); built["id"] = AnyEncodable(id)
        let args = built
        return try await performDetached { try $0.call("updateSkip", args) }
    }
    public func updateChangelog() async throws -> [UpdateRelease] {
        try await performDetached { try $0.call("updateChangelog", ["limit": AnyEncodable(10)]) }
    }
    public func updateDownloadStart(id: String) async throws {
        let args = versionArgs.merging(["id": AnyEncodable(id)]) { $1 }
        let _: LyricJSON = try await performDetached { try $0.call("updateDownloadStart", args) }
    }
    public func updateDownloadProgress() async throws -> JobProgress<String> {
        try await performDetached { try $0.call("updateDownloadProgress") }
    }
    public func updateInstall(appPath: String) async throws {
        let _: LyricJSON = try await performDetached { try $0.call("updateInstall", ["appPath": AnyEncodable(appPath)]) }
    }
    public func updateReleasePage(_ url: String) async throws -> String {
        try await performDetached { try $0.call("updateReleasePage", ["url": AnyEncodable(url)]) }
    }
    public func diagnosticsReport(audioAvailable: Bool) async throws -> String {
        var built = versionArgs
        built["system"] = AnyEncodable("macOS \(ProcessInfo.processInfo.operatingSystemVersionString)")
        built["audioAvailable"] = AnyEncodable(audioAvailable)
        let args = built // @Sendable 闭包只能捕获常量
        return try await performDetached { try $0.call("diagnosticsReport", args) }
    }
    public func diagnosticsSave(path: String, audioAvailable: Bool) async throws -> String {
        var built = versionArgs
        built["system"] = AnyEncodable("macOS \(ProcessInfo.processInfo.operatingSystemVersionString)")
        built["audioAvailable"] = AnyEncodable(audioAvailable); built["path"] = AnyEncodable(path)
        let args = built
        return try await performDetached { try $0.call("diagnosticsSave", args) }
    }
}

public struct LegacyDictionarySource: Codable, Identifiable, Sendable {
    public var name: String; public var path: String; public var kind: String; public var exists: Bool; public var imported: Bool
    public var id: String { path }
}
public struct ScrapeAttempt: Codable, Identifiable, Sendable {
    public var id: Int64; public var filePath: String; public var provider: String; public var status: String
    public var confidence: Double; public var errorType: String; public var errorMessage: String; public var attemptedAt: String
}
extension CorpusClient {
    public func dictLegacySources() async throws -> [LegacyDictionarySource] { try await perform { try $0.call("dictLegacySources") } }
    public func scrapeAttempts(_ path: String) async throws -> [ScrapeAttempt] {
        try await perform { try $0.call("scrapeAttempts", ["filePath": AnyEncodable(path)]) }
    }
    public func dictDefinitionHtml(_ definition: LookupDefinition) async throws -> String {
        try await performDetached { try $0.call("dictDefinitionHtml", ["dictionary": AnyEncodable(definition.dictionary), "entries": AnyEncodable(definition.entries ?? [])]) }
    }
}

public struct HealthReport: Codable, Sendable {
    public var dbPath: String; public var tracks: Int64; public var lyricLines: Int64
    public var tokenizerReady: Bool; public var audioReady: Bool
}
public struct WordFrequency: Codable, Identifiable, Sendable {
    public var lemma: String; public var pos: String; public var freq: Int64; public var songCount: Int64; public var surfaces: [String]
    public var id: String { lemma + "|" + pos }
}
extension CorpusClient {
    public func health(audioAvailable: Bool) async throws -> HealthReport {
        try await perform { try $0.call("health", ["audioAvailable": AnyEncodable(audioAvailable)]) }
    }
    public func wordFrequency(pos: String = "NOUN", limit: Int = 12) async throws -> [WordFrequency] {
        try await perform { try $0.call("wordFrequency", ["pos": AnyEncodable(pos), "limit": AnyEncodable(limit)]) }
    }
    public func scrapeAlbumFillStart() async throws -> Int { try await performDetached { try $0.call("scrapeAlbumFillStart") } }
}
