import Foundation

public struct LyricsDisplay: Codable, Equatable, Sendable {
    public var fontFamily = ""
    public var fallbackFamily = ""
    public var fontSize = 15.0
    public var rubySize = 9.0
    public var lineGap = 6.0
    public var letterSpacing = 0.0
    public init() {}
    public mutating func normalize() {
        fontSize = min(30, max(12, fontSize.isFinite ? fontSize.rounded() : 15))
        rubySize = min(18, max(6, rubySize.isFinite ? rubySize.rounded() : 9))
        lineGap = min(36, max(0, lineGap.isFinite ? lineGap.rounded() : 6))
        letterSpacing = min(12, max(0, letterSpacing.isFinite ? letterSpacing.rounded() : 0))
    }
}
