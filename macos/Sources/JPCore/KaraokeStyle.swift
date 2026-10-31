import Foundation

/// 卡拉 OK 字幕的样式：字体、字号、粗细，没唱 / 唱过两套「填色 + 描边 + 发光」，立体阴影，两行错开。
///
/// 字段名、预设、校验规则和 Windows 的 `app/src/karaokeStyle.ts` 一字不差——两边的测试读同一份
/// `Tests/JPCoreTests/Fixtures/KaraokeStyle.json`。颜色都是小写的 `#rrggbb`。
public struct KaraokeStyle: Codable, Equatable, Sendable {
    /// 空串：跟歌词字体走
    public var fontFamily: String
    /// 窗口够宽时的字号（pt）；窗口窄了按比例缩，描边、发光、阴影跟着缩
    public var fontSize: Double
    /// 400–900，100 一档
    public var fontWeight: Int
    public var unsungFill: String
    public var unsungStroke: String
    public var unsungGlow: String
    public var sungFill: String
    public var sungStroke: String
    public var sungGlow: String
    public var strokeWidth: Double
    /// 发光半径；0 不发光
    public var glowStrength: Double
    public var shadowColor: String
    /// 立体阴影往右下叠出去的厚度；0 没有
    public var shadowDepth: Double
    /// 上行靠左、下行靠右；关掉两行都居中
    public var staggered: Bool

    public enum Preset: String, CaseIterable, Identifiable, Sendable {
        case classic, pink, plain
        public var id: String { rawValue }
    }

    public static let presets: [Preset: KaraokeStyle] = [
        // 经典 KTV：白字细黑边、贴边一圈蓝光，唱过变蓝、白边，浅浅一层侧面
        .classic: .init(fontFamily: "", fontSize: 56, fontWeight: 900,
                        unsungFill: "#f4f4f4", unsungStroke: "#000000", unsungGlow: "#2a78ff",
                        sungFill: "#3a8cf5", sungStroke: "#ffffff", sungGlow: "#2a78ff",
                        strokeWidth: 3, glowStrength: 8, shadowColor: "#000000", shadowDepth: 2, staggered: true),
        // 粉色像素风 MV：白字粉紫边，唱过变粉、白边、粉色发光
        .pink: .init(fontFamily: "", fontSize: 56, fontWeight: 800,
                     unsungFill: "#ffffff", unsungStroke: "#d96bf0", unsungGlow: "#e070ff",
                     sungFill: "#ec4f95", sungStroke: "#ffffff", sungGlow: "#ff5fb0",
                     strokeWidth: 4, glowStrength: 16, shadowColor: "#6a2a86", shadowDepth: 3, staggered: true),
        // 原来的样子：没有描边，只有一圈深色投影
        .plain: .init(fontFamily: "", fontSize: 56, fontWeight: 700,
                      unsungFill: "#ffffff", unsungStroke: "#000000", unsungGlow: "#000000",
                      sungFill: "#4cc2ff", sungStroke: "#000000", sungGlow: "#000000",
                      strokeWidth: 0, glowStrength: 6, shadowColor: "#000000", shadowDepth: 0, staggered: true),
    ]

    /// 默认是原来的样子；KTV 风格的预设由用户自己挑
    public static var standard: KaraokeStyle { presets[.plain]! }

    public static let fontSizeRange: ClosedRange<Double> = 28...96
    public static let fontWeightRange: ClosedRange<Int> = 400...900
    public static let strokeWidthRange: ClosedRange<Double> = 0...12
    public static let glowStrengthRange: ClosedRange<Double> = 0...40
    public static let shadowDepthRange: ClosedRange<Double> = 0...12

    public init(fontFamily: String, fontSize: Double, fontWeight: Int,
                unsungFill: String, unsungStroke: String, unsungGlow: String,
                sungFill: String, sungStroke: String, sungGlow: String,
                strokeWidth: Double, glowStrength: Double, shadowColor: String, shadowDepth: Double, staggered: Bool) {
        self.fontFamily = fontFamily; self.fontSize = fontSize; self.fontWeight = fontWeight
        self.unsungFill = unsungFill; self.unsungStroke = unsungStroke; self.unsungGlow = unsungGlow
        self.sungFill = sungFill; self.sungStroke = sungStroke; self.sungGlow = sungGlow
        self.strokeWidth = strokeWidth; self.glowStrength = glowStrength
        self.shadowColor = shadowColor; self.shadowDepth = shadowDepth; self.staggered = staggered
    }

    /// 读回来的可能是旧版本、被手改过的：逐项校验，坏的项用默认值（和 Windows 的 normalizeKaraokeStyle 同一套规则）
    public static func normalized(_ object: Any?) -> KaraokeStyle {
        let raw = object as? [String: Any] ?? [:]
        let d = standard
        func number(_ key: String) -> Double? {
            // JSONSerialization 把 true/false 也读成 NSNumber：布尔不算数字
            guard let n = raw[key] as? NSNumber, CFGetTypeID(n) != CFBooleanGetTypeID() else { return nil }
            let v = n.doubleValue
            return v.isFinite ? v : nil
        }
        func clamp(_ key: String, _ range: ClosedRange<Double>, _ fallback: Double, step: Double = 1) -> Double {
            guard let v = number(key) else { return fallback }
            // JS 的 Math.round：.5 往正无穷进
            return min(range.upperBound, max(range.lowerBound, (v / step + 0.5).rounded(.down) * step))
        }
        func color(_ key: String, _ fallback: String) -> String {
            guard let s = raw[key] as? String, s.count == 7, s.hasPrefix("#"),
                  s.dropFirst().allSatisfy(\.isHexDigit) else { return fallback }
            return s.lowercased()
        }
        let family = (raw["fontFamily"] as? String).flatMap { $0.count <= 200 ? $0.trimmingCharacters(in: .whitespaces) : nil }
        let weightRange = Double(fontWeightRange.lowerBound)...Double(fontWeightRange.upperBound)
        return .init(
            fontFamily: family ?? d.fontFamily,
            fontSize: clamp("fontSize", fontSizeRange, d.fontSize),
            fontWeight: Int(clamp("fontWeight", weightRange, Double(d.fontWeight), step: 100)),
            unsungFill: color("unsungFill", d.unsungFill),
            unsungStroke: color("unsungStroke", d.unsungStroke),
            unsungGlow: color("unsungGlow", d.unsungGlow),
            sungFill: color("sungFill", d.sungFill),
            sungStroke: color("sungStroke", d.sungStroke),
            sungGlow: color("sungGlow", d.sungGlow),
            strokeWidth: clamp("strokeWidth", strokeWidthRange, d.strokeWidth),
            glowStrength: clamp("glowStrength", glowStrengthRange, d.glowStrength),
            shadowColor: color("shadowColor", d.shadowColor),
            shadowDepth: clamp("shadowDepth", shadowDepthRange, d.shadowDepth),
            staggered: (raw["staggered"] as? NSNumber).flatMap { CFGetTypeID($0) == CFBooleanGetTypeID() ? $0.boolValue : nil } ?? d.staggered
        )
    }

    public static func decode(_ data: Data?) -> KaraokeStyle {
        normalized(data.flatMap { try? JSONSerialization.jsonObject(with: $0) })
    }

    public func normalized() -> KaraokeStyle {
        let data = try? JSONEncoder().encode(self)
        return Self.decode(data)
    }

    /// 当前样式正好是哪套预设（字体、字号不算）
    public var matchingPreset: Preset? {
        Preset.allCases.first { id in
            var preset = Self.presets[id]!
            preset.fontFamily = fontFamily
            preset.fontSize = fontSize
            return preset == self
        }
    }

    /// 套用预设：颜色、描边、发光、阴影、粗细换掉，字体和字号留着
    public func applying(_ id: Preset) -> KaraokeStyle {
        var out = Self.presets[id]!
        out.fontFamily = fontFamily
        out.fontSize = fontSize
        return out
    }

    /// 立体侧面的几步位移：1、2、4… 翻倍叠，最后补零头（和 Windows 的 extrusionSteps 一样）。
    /// 每一步都把之前叠好的整块再挪一次，所以几步就能连成 0…厚度 的一整块
    public static func extrusionSteps(_ depth: Int) -> [Int] {
        var steps: [Int] = [], covered = 0, next = 1
        while covered + next <= depth { steps.append(next); covered += next; next *= 2 }
        if covered < depth { steps.append(depth - covered) }
        return steps
    }

    /// `#rrggbb` → 0...1 的 RGB
    public static func rgb(_ hex: String) -> (r: Double, g: Double, b: Double) {
        let v = UInt32(hex.dropFirst(), radix: 16) ?? 0xffffff
        return (Double((v >> 16) & 0xff) / 255, Double((v >> 8) & 0xff) / 255, Double(v & 0xff) / 255)
    }

    /// 0...1 的 RGB → `#rrggbb`
    public static func hex(r: Double, g: Double, b: Double) -> String {
        func byte(_ x: Double) -> Int { Int((min(1, max(0, x.isFinite ? x : 0)) * 255).rounded()) }
        return String(format: "#%02x%02x%02x", byte(r), byte(g), byte(b))
    }
}
