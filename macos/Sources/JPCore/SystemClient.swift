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
    public var id: String { tag }
    public var identity: String { buildId ?? version }
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
        var args = versionArgs; args["channel"] = AnyEncodable(channel)
        return try await performDetached { try $0.call("updateSetChannel", args) }
    }
    public func updateSkip(_ channel: UpdateChannel, id: String) async throws -> UpdatePreferences {
        var args = versionArgs; args["channel"] = AnyEncodable(channel); args["id"] = AnyEncodable(id)
        return try await performDetached { try $0.call("updateSkip", args) }
    }
    public func updateChangelog() async throws -> [UpdateRelease] {
        try await performDetached { try $0.call("updateChangelog", ["limit": AnyEncodable(10)]) }
    }
    public func updateReleasePage(_ url: String) async throws -> String {
        try await performDetached { try $0.call("updateReleasePage", ["url": AnyEncodable(url)]) }
    }
    public func diagnosticsReport(audioAvailable: Bool) async throws -> String {
        var args = versionArgs
        args["system"] = AnyEncodable("macOS \(ProcessInfo.processInfo.operatingSystemVersionString)")
        args["audioAvailable"] = AnyEncodable(audioAvailable)
        return try await performDetached { try $0.call("diagnosticsReport", args) }
    }
    public func diagnosticsSave(path: String, audioAvailable: Bool) async throws -> String {
        var args = versionArgs
        args["system"] = AnyEncodable("macOS \(ProcessInfo.processInfo.operatingSystemVersionString)")
        args["audioAvailable"] = AnyEncodable(audioAvailable); args["path"] = AnyEncodable(path)
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
