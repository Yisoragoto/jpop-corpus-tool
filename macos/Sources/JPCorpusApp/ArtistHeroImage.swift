import AppKit
import JPCorpusModel
import SwiftUI
import Vision

/// 音乐人头图：照片铺满横幅，按人脸（找不到脸用显著区域）决定裁哪一块。焦点按文件缓存，只算一次
@MainActor
struct ArtistHeroImage: View {
    @Environment(AppModel.self) private var app
    let path: String
    let size: CGSize
    @State private var image: NSImage?
    @State private var focus: CGPoint?

    var body: some View {
        ZStack(alignment: .topLeading) {
            Color.black.opacity(0.6)
            if let image {
                let placed = HeroFraming.place(image: image.size, in: size, focus: focus ?? CGPoint(x: 0.5, y: 0.3))
                Image(nsImage: image)
                    .resizable()
                    .frame(width: placed.size.width, height: placed.size.height)
                    .offset(x: placed.origin.x, y: placed.origin.y)
                    .animation(.smooth(duration: 0.4), value: focus)
                    .transition(.opacity)
            }
        }
        .frame(width: size.width, height: size.height, alignment: .topLeading)
        .clipped()
        .animation(.easeOut(duration: 0.3), value: image == nil)
        .task(id: "\(path)-\(app.library.version)") {
            image = nil
            focus = nil
            guard !path.isEmpty, let client = app.client else { return }
            let resolved = (try? await client.resolveMedia(path)) ?? nil
            guard let resolved, let decoded = await Artwork.image(atPath: resolved, maxPixel: 1600) else { return }
            guard !Task.isCancelled else { return }
            image = decoded
            focus = await HeroFocus.focus(path: resolved, image: decoded)
        }
    }
}

/// 照片里人在哪。先找脸，没有脸（背影、插画、乐队 logo）再用显著区域
@MainActor
enum HeroFocus {
    private static var cache: [String: CGPoint] = [:]

    static func focus(path: String, image: NSImage) async -> CGPoint? {
        if let hit = cache[path] { return hit }
        guard let cgImage = image.cgImage(forProposedRect: nil, context: nil, hints: nil) else { return nil }
        let found = await Task.detached(priority: .utility) { detect(cgImage) }.value
        if let found { cache[path] = found }
        return found
    }

    /// Vision 的坐标原点在左下，这里换成左上
    nonisolated private static func detect(_ image: CGImage) -> CGPoint? {
        let handler = VNImageRequestHandler(cgImage: image, options: [:])
        let faces = VNDetectFaceRectanglesRequest()
        try? handler.perform([faces])
        let boxes = (faces.results ?? []).map(\.boundingBox).map { flip($0) }
        // 很小的脸多半是背景里的路人或海报，只留下和最大那张差不多大的
        if let largest = boxes.map({ $0.width * $0.height }).max(), largest > 0 {
            let main = boxes.filter { $0.width * $0.height >= largest * 0.25 }
            if let point = HeroFraming.focus(faces: main) { return point }
        }
        let saliency = VNGenerateAttentionBasedSaliencyImageRequest()
        try? handler.perform([saliency])
        if let object = saliency.results?.first?.salientObjects?.first {
            let box = flip(object.boundingBox)
            return CGPoint(x: box.midX, y: box.minY + box.height * 0.35)
        }
        return nil
    }

    nonisolated private static func flip(_ r: CGRect) -> CGRect {
        CGRect(x: r.minX, y: 1 - r.maxY, width: r.width, height: r.height)
    }
}
