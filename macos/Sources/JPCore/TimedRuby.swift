import Foundation

/// 逐字歌词行加振假名。
///
/// 逐字行的每个单位是一个能点的按钮（从这个单位的真实起点放），振假名要压在原文对应的字上面。
/// 一段注音可能跨两个单位（整词注音「真夜中」、单位是一个个字），也可能只占单位的一部分
/// （注音「夜」、单位是「夜に」）。所以：
///
/// - **块**（`Cluster`）：换行只能在块之间。注音跨过的单位边界不能断开，否则读音和字会分到两行；
/// - **片**（`Piece`）：块里按注音切开，一片上面一段读音（或留空），下面是原文；
/// - **段**（`Segment`）：片再按单位切开，每段属于一个单位，点它从那个单位放。
///
/// 一个单位被切成几段时，每段的遮罩按它在单位里占的字数比例取，拼起来和整个单位一个遮罩一样，
/// 不另造逐字时间。
///
/// 以前逐字行根本不画振假名：曲库 239 首里 190 首有逐字时间，开着振假名大多数歌也看不到
public enum TimedRuby {
    public struct Segment: Hashable, Sendable {
        /// `units` 里的下标
        public var unit: Int
        public var text: String
        /// 这一段在所属单位里的位置（0…1，按字数）
        public var from: Double
        public var to: Double

        /// 单位进度为 `p` 时这一段亮了多少
        public func progress(unitProgress p: Double) -> Double {
            guard to > from else { return p >= 1 ? 1 : 0 }
            return min(1, max(0, (p - from) / (to - from)))
        }
    }

    public struct Piece: Hashable, Sendable {
        public var reading: String?
        public var segments: [Segment]
    }

    public struct Cluster: Hashable, Sendable, Identifiable {
        /// 第一个字的位置
        public var id: Int
        public var pieces: [Piece]
    }

    /// 没有可用的注音时返回 nil（照原样一个单位一个按钮地画）
    public static func clusters(text: String, units: [LyricTimingUnit], rubies: [Ruby]) -> [Cluster]? {
        let scalars = Array(text.unicodeScalars)
        // 只要落在原文里、彼此不重叠的注音
        var valid: [Ruby] = []
        for ruby in rubies.sorted(by: { $0.start < $1.start }) {
            guard ruby.start >= 0, ruby.start < ruby.end, ruby.end <= scalars.count, !ruby.reading.isEmpty,
                  ruby.start >= (valid.last?.end ?? 0) else { continue }
            valid.append(ruby)
        }
        guard !valid.isEmpty else { return nil }

        func rubyIndex(at position: Int) -> Int? {
            valid.firstIndex { $0.start <= position && position < $0.end }
        }
        /// 这个位置是不是在某段注音中间（不能从这里断开）
        func inside(_ position: Int) -> Bool {
            valid.contains { $0.start < position && position < $0.end }
        }

        var out: [Cluster] = []
        var current: Cluster?
        // 当前这一片属于哪段注音（nil = 没有注音的字）
        var currentRuby: Int?

        for (index, unit) in units.enumerated() {
            let start = unit.charStart, end = unit.charEnd
            guard start >= 0, start < end, end <= scalars.count else { continue }
            // 单位里按注音的边界切开
            var cuts = [start, end]
            for ruby in valid {
                if ruby.start > start && ruby.start < end { cuts.append(ruby.start) }
                if ruby.end > start && ruby.end < end { cuts.append(ruby.end) }
            }
            cuts = Array(Set(cuts)).sorted()
            let length = Double(end - start)
            for (a, b) in zip(cuts, cuts.dropFirst()) {
                let owner = rubyIndex(at: a)
                let segment = Segment(unit: index,
                                      text: String(String.UnicodeScalarView(scalars[a..<b])),
                                      from: Double(a - start) / length, to: Double(b - start) / length)
                if current == nil { current = Cluster(id: a, pieces: []) }
                if let last = current?.pieces.indices.last, currentRuby == owner {
                    current?.pieces[last].segments.append(segment)
                } else {
                    current?.pieces.append(Piece(reading: owner.map { valid[$0].reading }, segments: [segment]))
                    currentRuby = owner
                }
            }
            // 单位的末尾不在注音中间：这里可以换行
            if !inside(end), let done = current {
                out.append(done)
                current = nil
                currentRuby = nil
            }
        }
        if let done = current { out.append(done) }
        return out
    }
}
