import Foundation

/// 一键刮削时作业进度的 stage：Rust 写成「第几步/共几步|种类|标题」（jp-ffi 的 start_scrape），这里拆开。
/// 单项作业的 stage 就是标题本身，拆不出来时返回 nil
public struct ScrapeStage: Equatable, Sendable {
    public var step: Int
    public var steps: Int
    /// track / artist / cover / album
    public var kind: String
    public var title: String

    public init?(_ stage: String) {
        let parts = stage.split(separator: "|", maxSplits: 2, omittingEmptySubsequences: false)
        guard parts.count == 3 else { return nil }
        let counts = parts[0].split(separator: "/")
        guard counts.count == 2, let step = Int(counts[0]), let steps = Int(counts[1]), steps > 0 else { return nil }
        self.step = step
        self.steps = steps
        kind = String(parts[1])
        title = String(parts[2])
    }

    /// 这一步做的是什么（中文原文，界面上再过 L10n）
    public var label: String {
        switch kind {
        case "track": "识别曲目"
        case "artist": "歌手照片"
        case "cover": "补封面"
        case "album": "专辑信息"
        default: kind
        }
    }
}
