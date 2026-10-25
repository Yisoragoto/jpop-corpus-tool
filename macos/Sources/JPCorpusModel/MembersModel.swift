import Foundation
import JPCore
import Observation

@MainActor @Observable
public final class MembersModel {
    public var members: [Membership] = []
    public var bands: [Membership] = []
    public var suggestions: [MembershipSuggestion] = []
    public var removal: Int64?
    public var busy = false
    public var errorMessage: String?
    public var note = ""
    public init() {}
    public func load(_ id: Int64, client: any LibraryCalling) async {
        errorMessage = nil
        do {
            async let m = client.bandMembers(id)
            async let b = client.bandsOf(id)
            async let s = client.membershipSuggestions()
            let (loadedMembers, loadedBands, loadedSuggestions) = try await (m, b, s)
            members = loadedMembers; bands = loadedBands
            suggestions = loadedSuggestions.filter { $0.bandId == id }
        } catch { errorMessage = error.localizedDescription }
    }
    @discardableResult public func put(_ input: MembershipInput, client: any LibraryCalling) async -> Bool {
        var input = input
        input.part = input.part.trimmingCharacters(in: .whitespacesAndNewlines)
        input.joined = input.joined.trimmingCharacters(in: .whitespacesAndNewlines)
        input.leftAt = input.leftAt.trimmingCharacters(in: .whitespacesAndNewlines)
        input.memberName = input.memberName?.trimmingCharacters(in: .whitespacesAndNewlines)
        guard input.memberId != nil || !(input.memberName ?? "").isEmpty else { errorMessage = "请填写新成员名字"; return false }
        guard LibraryIndex.validDate(input.joined.trimmingCharacters(in: .whitespacesAndNewlines)), LibraryIndex.validDate(input.leftAt.trimmingCharacters(in: .whitespacesAndNewlines)) else { errorMessage = "日期请写 YYYY、YYYY-MM、YYYY-MM-DD 或 ?"; return false }
        return await run(input.bandId, client: client) { try await client.membershipPut(input).note }
    }
    public func confirm(_ band: Int64, member: Int64, client: any LibraryCalling) async {
        _ = await run(band, client: client) { try await client.membershipConfirm(bandId: band, memberId: member).note }
    }
    public func rejectSuggestion(_ band: Int64, member: Int64, client: any LibraryCalling) async {
        _ = await run(band, client: client) { try await client.membershipReject(bandId: band, memberId: member); return "" }
    }
    public func removeConfirmed(_ band: Int64, client: any LibraryCalling) async {
        guard let member = removal else { return }
        _ = await run(band, client: client) { try await client.membershipReject(bandId: band, memberId: member); return "" }; removal = nil
    }
    private func run(_ band: Int64, client: any LibraryCalling, work: () async throws -> String) async -> Bool {
        guard !busy else { return false }; busy = true; errorMessage = nil; defer { busy = false }
        do { note = try await work(); await load(band, client: client); return true } catch { errorMessage = error.localizedDescription; return false }
    }
}
