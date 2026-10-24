import Foundation

extension CorpusClient {
    public func lemmaForms(_ lemmas: [String]) async throws -> [LemmaForm] {
        try await performDetached { try $0.call("lemmaForms", ["lemmas": AnyEncodable(lemmas)]) }
    }
    public func libraryEditSong(_ songId: String, edit: SongEdit) async throws -> EditReport {
        return try await performDetached { try $0.call("libraryEditSong", ["songId": AnyEncodable(songId), "edit": AnyEncodable(edit)]) }
    }
    public func libraryDeleteSong(_ songId: String) async throws -> DeleteResult {
        return try await performDetached { try $0.call("libraryDeleteSong", ["songId": AnyEncodable(songId)]) }
    }
    public func libraryMissingAudio() async throws -> [MissingAudio] {
        return try await performDetached { try $0.call("libraryMissingAudio") }
    }
    public func librarySuggestRelinks(_ folder: String) async throws -> [RelinkSuggestion] {
        return try await performDetached { try $0.call("librarySuggestRelinks", ["folder": AnyEncodable(folder)]) }
    }
    public func libraryRelinkAudio(_ links: [RelinkRequest]) async throws -> RelinkBatch {
        return try await performDetached { try $0.call("libraryRelinkAudio", ["links": AnyEncodable(links)]) }
    }
    public func libraryNewAudio(ignoredFolders: [String], ignoredFiles: [String]) async throws -> NewAudio {
        return try await performDetached { try $0.call("libraryNewAudio", ["ignoredFolders": AnyEncodable(ignoredFolders), "ignoredFiles": AnyEncodable(ignoredFiles)]) }
    }
    public func artistImageSet(_ name: String, path: String) async throws -> String {
        return try await performDetached { try $0.call("artistImageSet", ["name": AnyEncodable(name), "path": AnyEncodable(path)]) }
    }
    public func artistImageReset(_ name: String) async throws -> String {
        return try await performDetached { try $0.call("artistImageReset", ["name": AnyEncodable(name)]) }
    }
    public func artistRoster() async throws -> [ArtistRow] {
        return try await performDetached { try $0.call("artistRoster") }
    }
    public func scrapeArtist(_ name: String) async throws -> ArtistOutcome {
        try await performDetached { try $0.call("scrapeArtist", ["name": AnyEncodable(name)]) }
    }
    public func memberships() async throws -> [Membership] {
        return try await performDetached { try $0.call("memberships") }
    }
    public func bandMembers(_ personId: Int64) async throws -> [Membership] {
        return try await performDetached { try $0.call("bandMembers", ["personId": AnyEncodable(personId)]) }
    }
    public func bandsOf(_ personId: Int64) async throws -> [Membership] {
        return try await performDetached { try $0.call("bandsOf", ["personId": AnyEncodable(personId)]) }
    }
    public func membershipSuggestions() async throws -> [MembershipSuggestion] {
        return try await performDetached { try $0.call("membershipSuggestions") }
    }
    public func membershipPut(_ input: MembershipInput) async throws -> MembershipPutOutcome {
        return try await performDetached { try $0.call("membershipPut", ["bandId": AnyEncodable(input.bandId), "memberId": AnyEncodable(input.memberId), "memberName": AnyEncodable(input.memberName), "part": AnyEncodable(input.part), "joined": AnyEncodable(input.joined), "leftAt": AnyEncodable(input.leftAt)]) }
    }
    public func membershipConfirm(bandId: Int64, memberId: Int64) async throws -> MembershipPutOutcome {
        return try await performDetached { try $0.call("membershipConfirm", ["bandId": AnyEncodable(bandId), "memberId": AnyEncodable(memberId)]) }
    }
    public func membershipReject(bandId: Int64, memberId: Int64) async throws {
        try await performDetached { try $0.run("membershipReject", ["bandId": AnyEncodable(bandId), "memberId": AnyEncodable(memberId)]) }
    }
    public func tokenCorrection(_ utteranceId: Int64) async throws -> CorrectionView? {
        return try await performDetached { try $0.call("tokenCorrection", ["utteranceId": AnyEncodable(utteranceId)]) }
    }
    public func saveTokenCorrection(_ utteranceId: Int64, tokens: [TokenEdit]) async throws {
        try await performDetached { try $0.run("saveTokenCorrection", ["utteranceId": AnyEncodable(utteranceId), "tokens": AnyEncodable(tokens)]) }
    }
    public func revertTokenCorrection(_ utteranceId: Int64) async throws -> Bool {
        return try await performDetached { try $0.call("revertTokenCorrection", ["utteranceId": AnyEncodable(utteranceId)]) }
    }
    public func peopleByRole(_ role: String, limit: Int) async throws -> [PersonSummary] {
        return try await performDetached { try $0.call("peopleByRole", ["role": AnyEncodable(role), "limit": AnyEncodable(limit)]) }
    }
    public func worksByPerson(_ personId: Int64) async throws -> [Track] {
        return try await performDetached { try $0.call("worksByPerson", ["personId": AnyEncodable(personId)]) }
    }
    public func mostPlayed(minListenedSec: Double, limit: Int) async throws -> [PlayedTrack] {
        return try await performDetached { try $0.call("mostPlayed", ["minListenedSec": AnyEncodable(minListenedSec), "limit": AnyEncodable(limit)]) }
    }
    public func recentlyPlayed(limit: Int) async throws -> [PlayedTrack] {
        return try await performDetached { try $0.call("recentlyPlayed", ["limit": AnyEncodable(limit)]) }
    }
    public func migratePlan(_ path: String) async throws -> MigratePlan {
        return try await performDetached { try $0.call("migratePlan", ["path": AnyEncodable(path)]) }
    }
    public func migrateRun(_ path: String, options: MigrateOptions) async throws {
        try await performDetached { try $0.run("migrateRun", ["path": AnyEncodable(path), "options": AnyEncodable(options)]) }
    }
    public func backfillDurations() async throws {
        try await performDetached { try $0.run("backfillDurations") }
    }
    public func fillAlbumArtwork() async throws {
        try await performDetached { try $0.run("fillAlbumArtwork") }
    }
    public func libraryProgress() async throws -> JobProgress<LibraryJobResult> {
        return try await performDetached { try $0.call("libraryProgress") }
    }
}
