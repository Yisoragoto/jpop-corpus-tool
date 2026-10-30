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
    /// 上次检查的时间和失败原因（nil = 成功）。启动时的自动检查失败不弹窗打扰，
    /// 但设置里要看得出「没联网 / 清单坏了」和「已经是最新」不是一回事
    public private(set) var lastCheckedAt: Date? { didSet { defaults.set(lastCheckedAt, forKey: "updateLastCheckedAt") } }
    public private(set) var lastCheckError: String? { didSet { defaults.set(lastCheckError, forKey: "updateLastCheckError") } }
    /// `status` 是哪一次检查得到的。之后的检查失败不清掉它（还能点「查看新版本」），
    /// 但界面要说明它是那次成功检查的结果，不是这一次的
    public private(set) var statusCheckedAt: Date?
    /// 最近一次检查失败了，手上的 `status` 是更早那次成功检查留下的
    public var statusIsStale: Bool { status != nil && lastCheckError != nil }
    /// 正在下载（0~1；总大小未知时是 nil 但 `downloading` 为真）
    public private(set) var downloading = false
    public private(set) var downloadFraction: Double?
    /// 替换脚本已经拉起，马上退出
    public private(set) var installing = false
    /// 这一版能不能在程序里直接装
    public var canInstallInApp: Bool { status?.updateAvailable == true && status?.latest?.installer != nil }
    @ObservationIgnored public var pollInterval: Duration = .milliseconds(300)
    @ObservationIgnored private let defaults: UserDefaults
    private var started = false
    public init(defaults: UserDefaults = .standard, client: (any UpdateCalling)? = nil) {
        self.defaults = defaults; self.client = client
        autoCheck = defaults.object(forKey: "autoCheckUpdates") as? Bool ?? true
        lastCheckedAt = defaults.object(forKey: "updateLastCheckedAt") as? Date
        lastCheckError = defaults.string(forKey: "updateLastCheckError")
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
            let now = Date(); lastCheckedAt = now; statusCheckedAt = now; lastCheckError = nil
            if automatic && result.updateAvailable && !result.skipped { offerSkip = true; showDialog = true }
        } catch {
            lastCheckedAt = Date(); lastCheckError = error.localizedDescription
            if !automatic { errorMessage = error.localizedDescription }
        }
    }
    public func setChannel(_ channel: UpdateChannel) async {
        guard !busy, let client else { return }; busy = true; errorMessage = nil
        defer { busy = false }
        do { preferences = try await client.updateSetChannel(channel); status = nil; statusCheckedAt = nil; showDialog = false }
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
    /// 下载刚才检查到的那一版、校验，然后拉起替换脚本并调用 `quit` 退出。
    /// 失败停在原地，把原因放进 `errorMessage`，旧版不受影响
    public func downloadAndInstall(appPath: String, quit: @MainActor () -> Void) async {
        guard !busy, !downloading, let client, let release = status?.latest, canInstallInApp else { return }
        downloading = true; downloadFraction = nil; errorMessage = nil
        defer { downloading = false }
        do {
            try await client.updateDownloadStart(id: release.identity)
            while true {
                let progress = try await client.updateDownloadProgress()
                downloadFraction = progress.fraction
                if progress.finished {
                    if let error = progress.error { throw CoreError.call(method: "updateDownloadProgress", message: error) }
                    break
                }
                try await Task.sleep(for: pollInterval)
            }
            try await client.updateInstall(appPath: appPath)
            installing = true
            quit()
        } catch is CancellationError {
        } catch { errorMessage = error.localizedDescription }
    }
    public func changelog() async {
        guard !busy, let client else { return }; busy = true
        defer { busy = false }
        do { releases = try await client.updateChangelog(); showChangelog = true }
        catch { errorMessage = error.localizedDescription }
    }
}
