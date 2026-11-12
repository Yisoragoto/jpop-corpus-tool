import Foundation

/// 循环调整栏（#64）上的数字，和 Windows 的 LoopAdjustBar 同样的写法
public enum LoopFormat {
    /// 0:12.3 —— 微调的粒度是 0.1 秒，显示也到 0.1 秒
    public static func time(_ sec: Double) -> String {
        let tenths = Int((max(0, sec) * 10).rounded())
        let minutes = tenths / 600
        let rest = Double(tenths % 600) / 10
        return "\(minutes):" + String(format: "%04.1f", rest)
    }

    /// +0.3 / −0.2：调过多少（减号是 U+2212）。没调过是空
    public static func delta(_ ms: Int) -> String {
        guard ms != 0 else { return "" }
        return (ms > 0 ? "+" : "\u{2212}") + String(format: "%.1f", Double(abs(ms)) / 1000)
    }

    /// 按钮上的步长：−0.5 / −0.1 / +0.1 / +0.5
    public static func step(_ ms: Int) -> String {
        (ms > 0 ? "+" : "\u{2212}") + String(format: "%g", Double(abs(ms)) / 1000)
    }
}
