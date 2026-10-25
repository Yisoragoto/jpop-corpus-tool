import Foundation
import JPCore
import Observation

@MainActor @Observable
public final class UpdateModel {
    public var client: (any UpdateCalling)?
    public private(set) var preferences: UpdatePreferences?
    public private(set) var status: UpdateStatus?
    public private(set) var releases: [UpdateRelease] = []
    public private(set) var busy = false
    public var errorMessage: String?
    public var showDialog = false
    public var offerSkip = false
    public var showChangelog = false
    public var confirmDebug = false
    public var autoCheck: Bool { didSet { defaults.set(autoCheck, forKey: "autoCheckUpdates") } }
    @ObservationIgnored private let defaults: UserDefaults
    private var started = false
    public init(defaults: UserDefaults = .standard, client: (any UpdateCalling)? = nil) {
        self.defaults = defaults; self.client = client
        autoCheck = defaults.object(forKey: "autoCheckUpdates") as? Bool ?? true
    }
    public func load() async {
        do { preferences = try await client?.updatePreferences() }
        catch { errorMessage = error.localizedDescription }
    }
    public func startup() async {
        guard !started, client != nil else { return }; started = true
        await load()
        if autoCheck { await check(automatic: true) }
    }
    public func check(automatic: Bool = false) async {
        guard !busy, let client else { return }; busy = true; errorMessage = nil
        defer { busy = false }
        do {
            let result = try await client.updateCheck(); status = result
            if automatic && result.updateAvailable && !result.skipped { offerSkip = true; showDialog = true }
        } catch { if !automatic { errorMessage = error.localizedDescription } }
    }
    public func setChannel(_ channel: UpdateChannel) async {
        guard !busy, let client else { return }; busy = true; errorMessage = nil
        defer { busy = false }
        do { preferences = try await client.updateSetChannel(channel); status = nil; showDialog = false }
        catch { errorMessage = error.localizedDescription }
    }
    public func skip() async {
        guard !busy, let client, let status, let release = status.latest else { return }
        busy = true; defer { busy = false }
        do { preferences = try await client.updateSkip(status.channel, id: release.identity); showDialog = false }
        catch { errorMessage = error.localizedDescription }
    }
    public func clearSkipped() async {
        guard !busy, let client, let preferences else { return }; busy = true
        defer { busy = false }
        do { self.preferences = try await client.updateSkip(preferences.channel, id: "") }
        catch { errorMessage = error.localizedDescription }
    }
    public func changelog() async {
        guard !busy, let client else { return }; busy = true
        defer { busy = false }
        do { releases = try await client.updateChangelog(); showChangelog = true }
        catch { errorMessage = error.localizedDescription }
    }
}
