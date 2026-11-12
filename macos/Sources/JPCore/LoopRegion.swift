import Foundation

/// 循环区间（#64，Rust 的 `jp_corpus::segments::LoopRegion`）：从第一行开始到最后一行最后一个字唱完，
/// 再加上记住的微调。起点的微调记在第一行，终点的记在最后一行；试听和制卡截的那一段也用它。
public struct LoopRegion: Codable, Hashable, Sendable {
    public var songId: String
    public var firstUtteranceId: Int64
    public var lastUtteranceId: Int64
    /// 交给播放器的起止（含微调，夹在曲目范围内）
    public var startSec: Double
    public var endSec: Double
    /// 不含微调的起止
    public var baseStartSec: Double
    public var baseEndSec: Double
    /// 毫秒，负数是提前
    public var startDeltaMs: Int
    public var endDeltaMs: Int
    /// 终点取的是最后一个字的真实结束时刻
    public var endFromWords: Bool

    public var adjusted: Bool { startDeltaMs != 0 || endDeltaMs != 0 }
}

extension CorpusClient {
    /// 两行不在同一首、或者有一行没有时间戳时是 nil
    public func loopRegion(first: Int64, last: Int64) async throws -> LoopRegion? {
        try await perform {
            try $0.call("loopRegion", ["firstUtteranceId": AnyEncodable(first), "lastUtteranceId": AnyEncodable(last)])
        }
    }

    /// 记住这一段的微调。两个数是总量（不是在原来的上面加），0/0 是清掉；Rust 会夹在 ±10 秒内
    public func setLoopAdjust(first: Int64, last: Int64, startDeltaMs: Int, endDeltaMs: Int) async throws -> LoopRegion? {
        try await perform {
            try $0.call("loopAdjustSet", [
                "firstUtteranceId": AnyEncodable(first), "lastUtteranceId": AnyEncodable(last),
                "startDeltaMs": AnyEncodable(startDeltaMs), "endDeltaMs": AnyEncodable(endDeltaMs),
            ])
        }
    }
}
