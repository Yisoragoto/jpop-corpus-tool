import JPCore

public protocol LibraryCalling: AnyObject, Sendable {
    func libraryEditSong(_ songId: String, edit: SongEdit) async throws -> EditReport
    func libraryDeleteSong(_ songId: String) async throws -> DeleteResult
    func libraryMissingAudio() async throws -> [MissingAudio]
    func librarySuggestRelinks(_ folder: String) async throws -> [RelinkSuggestion]
    func libraryRelinkAudio(_ links: [RelinkRequest]) async throws -> RelinkBatch
    func libraryNewAudio(ignoredFolders: [String], ignoredFiles: [String]) async throws -> NewAudio
    func artistImageSet(_ name: String, path: String) async throws -> String
    func artistImageReset(_ name: String) async throws -> String
    func artistRoster() async throws -> [ArtistRow]
    func memberships() async throws -> [Membership]
    func bandMembers(_ personId: Int64) async throws -> [Membership]
    func bandsOf(_ personId: Int64) async throws -> [Membership]
    func membershipSuggestions() async throws -> [MembershipSuggestion]
    func membershipPut(_ input: MembershipInput) async throws -> MembershipPutOutcome
    func membershipConfirm(bandId: Int64, memberId: Int64) async throws -> MembershipPutOutcome
    func membershipReject(bandId: Int64, memberId: Int64) async throws
    func tokenCorrection(_ utteranceId: Int64) async throws -> CorrectionView?
    func saveTokenCorrection(_ utteranceId: Int64, tokens: [TokenEdit]) async throws
    func revertTokenCorrection(_ utteranceId: Int64) async throws -> Bool
    func peopleByRole(_ role: String, limit: Int) async throws -> [PersonSummary]
    func worksByPerson(_ personId: Int64) async throws -> [Track]
    func mostPlayed(minListenedSec: Double, limit: Int) async throws -> [PlayedTrack]
    func recentlyPlayed(limit: Int) async throws -> [PlayedTrack]
    func migratePlan(_ path: String) async throws -> MigratePlan
    func migrateRun(_ path: String, options: MigrateOptions) async throws
    func backfillDurations() async throws
    func fillAlbumArtwork() async throws
    func libraryProgress() async throws -> JobProgress<LibraryJobResult>
}

extension CorpusClient: LibraryCalling {}
