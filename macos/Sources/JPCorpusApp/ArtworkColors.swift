import AppKit
import SwiftUI

/// 与 Windows coverColor 一样，在 24×24 缩略图里挑饱和且不过暗的主色。
@MainActor
struct ArtworkColors {
    var backdrop = Color(white: 0.1)
    var highlight = Color(white: 0.8)
    var secondary = Color(white: 0.22)

    init(image: NSImage? = nil) {
        guard let image, let cgImage = image.cgImage(forProposedRect: nil, context: nil, hints: nil) else { return }
        var pixels = [UInt8](repeating: 0, count: 24 * 24 * 4)
        let drawn = pixels.withUnsafeMutableBytes { bytes -> Bool in
            guard let context = CGContext(data: bytes.baseAddress, width: 24, height: 24, bitsPerComponent: 8,
                                          bytesPerRow: 96, space: CGColorSpaceCreateDeviceRGB(),
                                          bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else { return false }
            context.draw(cgImage, in: CGRect(x: 0, y: 0, width: 24, height: 24))
            return true
        }
        guard drawn else { return }
        var chosen = [Double](repeating: 0, count: 3)
        var average = chosen
        var weight: Double = 0
        var total: Double = 0
        for index in stride(from: 0, to: pixels.count, by: 4) where pixels[index + 3] >= 128 {
            let rgb = (0..<3).map { Double(pixels[index + $0]) / 255 }
            for channel in 0..<3 { average[channel] += rgb[channel] }
            total += 1
            let high = rgb.max() ?? 0, low = rgb.min() ?? 0
            guard high >= 30.0 / 255, low <= 225.0 / 255 else { continue }
            let saturation = (high - low) / high
            let score = saturation * saturation * high
            if score > weight { weight = score; chosen = rgb }
        }
        guard total > 0 else { return }
        average = average.map { $0 / total }
        if weight <= 0.02 { chosen = average }
        let high = max(chosen.max() ?? 0, 0.001)
        let dark = chosen.map { $0 * min(1, 0.26 / high) }
        let bright = chosen.map { min(1, $0 * 0.92 / high) * 0.75 + 0.25 }
        backdrop = Color(red: dark[0], green: dark[1], blue: dark[2])
        highlight = Color(red: bright[0], green: bright[1], blue: bright[2])
        secondary = Color(red: average[0], green: average[1], blue: average[2])
    }
}
