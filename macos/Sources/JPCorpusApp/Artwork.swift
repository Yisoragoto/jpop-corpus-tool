import AppKit
import CoreImage
import ImageIO

/// 封面：后台解码成合适的尺寸，虚化版本也在后台算好一次，按路径缓存。
///
/// 以前播放条、歌词面板、全屏歌词各自 `NSImage(contentsOfFile:)` 读原图（常是几千像素的 JPEG），
/// 再在视图上挂 `.blur(radius: 50…120)`：模糊是窗口合成时每一帧现算的，进度条、频谱、歌词一动，
/// 整块就要重新模糊一遍——这就是播放时 CPU 四十几、WindowServer 占一半 GPU 的主要来源。
/// 现在模糊只算一次，界面上放的是一张静态图。
@MainActor
enum Artwork {
    private static let images = NSCache<NSString, NSImage>()

    /// 解码成最长边不超过 `maxPixel` 像素的图。读不到是 nil
    static func image(atPath path: String?, maxPixel: Int = 1024) async -> NSImage? {
        guard let path, !path.isEmpty else { return nil }
        let key = "\(stamp(path))|\(maxPixel)" as NSString
        if let hit = images.object(forKey: key) { return hit }
        let decoded = await Task.detached(priority: .userInitiated) { decode(path, maxPixel: maxPixel) }.value
        guard let decoded else { return nil }
        let image = NSImage(cgImage: decoded, size: NSSize(width: decoded.width, height: decoded.height))
        images.setObject(image, forKey: key)
        return image
    }

    /// 预先虚化好的封面。`radius` 是铺到 `displayWidth` 宽时想要的模糊半径（点），
    /// 在小图上按比例换算，所以虚化的观感和原来挂在视图上的 `.blur` 一样
    static func blurred(atPath path: String?, radius: Double, saturation: Double, displayWidth: Double) async -> NSImage? {
        guard let path, !path.isEmpty else { return nil }
        let key = "\(stamp(path))|blur|\(Int(radius))|\(saturation)|\(Int(displayWidth))" as NSString
        if let hit = images.object(forKey: key) { return hit }
        let made = await Task.detached(priority: .utility) {
            blur(path, radius: radius, saturation: saturation, displayWidth: displayWidth)
        }.value
        guard let made else { return nil }
        let image = NSImage(cgImage: made, size: NSSize(width: made.width, height: made.height))
        images.setObject(image, forKey: key)
        return image
    }
}

/// 缓存键带上文件的修改时间和大小：刮削换了封面（路径不变）也会重读
@MainActor private func stamp(_ path: String) -> String {
    let attributes = try? FileManager.default.attributesOfItem(atPath: path)
    let modified = (attributes?[.modificationDate] as? Date)?.timeIntervalSince1970 ?? 0
    let size = (attributes?[.size] as? NSNumber)?.int64Value ?? 0
    return "\(path)|\(modified)|\(size)"
}

/// 小图上做虚化：模糊半径和图一起缩，效果一样，开销小两个数量级
private let blurSide = 256

private func decode(_ path: String, maxPixel: Int) -> CGImage? {
    guard let source = CGImageSourceCreateWithURL(URL(fileURLWithPath: path) as CFURL, nil) else { return nil }
    let options: [CFString: Any] = [
        kCGImageSourceCreateThumbnailFromImageAlways: true,
        kCGImageSourceCreateThumbnailWithTransform: true,
        kCGImageSourceShouldCacheImmediately: true,
        kCGImageSourceThumbnailMaxPixelSize: maxPixel,
    ]
    return CGImageSourceCreateThumbnailAtIndex(source, 0, options as CFDictionary)
}

private func blur(_ path: String, radius: Double, saturation: Double, displayWidth: Double) -> CGImage? {
    guard let small = decode(path, maxPixel: blurSide) else { return nil }
    let input = CIImage(cgImage: small)
    let extent = input.extent
    // 显示时图会按「填满」放大到大约 displayWidth 宽：把想要的半径换算到小图的像素上
    let scale = Double(max(extent.width, extent.height)) / max(1, displayWidth)
    let sourceRadius = max(1, radius * scale)
    let output = input.clampedToExtent()
        .applyingGaussianBlur(sigma: sourceRadius / 2)
        .applyingFilter("CIColorControls", parameters: [kCIInputSaturationKey: saturation])
        .cropped(to: extent)
    let context = CIContext(options: [.cacheIntermediates: false])
    return context.createCGImage(output, from: extent)
}
