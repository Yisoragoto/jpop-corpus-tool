import AppKit
import JPCore
import SwiftUI
import JPCorpusModel

/// 封面 / 头像。库里记的是导入那台机器上的路径，先经 Rust 的 resolveMedia 找到本机的文件
@MainActor
struct CoverImage: View {
    @Environment(AppModel.self) private var app
    let path: String
    var size: CGFloat = 48
    var cornerRadius: CGFloat = 8
    var circle = false
    var placeholder = "music.note"
    var identity = ""
    @State private var image: NSImage?

    var body: some View {
        Group {
            if let image {
                Image(nsImage: image).resizable().aspectRatio(contentMode: .fill)
            } else {
                ZStack {
                    Rectangle().fill(.quaternary)
                    if circle && !identity.isEmpty {
                        let tones: [Color] = [.indigo, .teal, .orange, .pink]
                        tones[AvatarIdentity.avatarTone(identity)].opacity(0.18)
                        Text(AvatarIdentity.monogram(identity)).font(.system(size: size * 0.32, weight: .semibold))
                    } else {
                        Image(systemName: placeholder).font(.system(size: size * 0.32)).foregroundStyle(.secondary)
                    }
                }
            }
        }
        .frame(width: size, height: size)
        .clipShape(circle ? AnyShape(Circle()) : AnyShape(RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)))
        .task(id: "\(path)-\(app.library.version)") {
            image = nil
            guard !path.isEmpty, let client = app.client else { return }
            let resolved = (try? await client.resolveMedia(path)) ?? nil
            // 后台解码成显示尺寸（Retina 两倍）：曲库、首页一屏几十张封面，主线程上读原图会卡
            let decoded = await Artwork.image(atPath: resolved, maxPixel: Int(size * 2))
            guard !Task.isCancelled else { return }
            image = decoded
        }
    }
}

/// 一块区域的小标题
struct SectionHeader: View {
    let title: String
    var detail: String?

    var body: some View {
        HStack(alignment: .firstTextBaseline) {
            Text(title).font(.title3.weight(.semibold))
            if let detail {
                Text(detail).font(.callout).foregroundStyle(.secondary)
            }
            Spacer()
        }
    }
}

/// 封面卡片：首页的最近播放、收藏，曲库的专辑墙都用它
@MainActor
struct CoverTile: View {
    let coverPath: String
    let title: String
    let subtitle: String
    var size: CGFloat = 132
    var circle = false
    var placeholder = "music.note"
    let action: () -> Void
    @State private var hovering = false

    var body: some View {
        Button(action: action) {
            VStack(alignment: circle ? .center : .leading, spacing: 6) {
                CoverImage(path: coverPath, size: size, cornerRadius: 10, circle: circle, placeholder: placeholder, identity: title)
                    .shadow(color: .black.opacity(hovering ? 0.28 : 0.16), radius: hovering ? 10 : 6, y: 3)
                    .scaleEffect(hovering ? 1.03 : 1)
                Text(title).font(.callout.weight(.medium)).lineLimit(1)
                Text(subtitle).font(.caption).foregroundStyle(.secondary).lineLimit(1)
            }
            .frame(width: size)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .onHover { h in withAnimation(.easeOut(duration: 0.15)) { hovering = h } }
    }
}

/// 一个大数字加一行说明
struct StatBlock: View {
    let value: String
    let label: String

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(value).font(.title2.weight(.semibold)).monospacedDigit()
            Text(label).font(.caption).foregroundStyle(.secondary)
        }
    }
}

/// 「12 分钟」「1 小时 5 分」
func formatListening(_ seconds: Double) -> String {
    if seconds > 0 && seconds < 60 { return "不到 1 分钟" }
    let minutes = Int((seconds / 60).rounded())
    if minutes < 60 { return "\(minutes) 分钟" }
    return minutes % 60 == 0 ? "\(minutes / 60) 小时" : "\(minutes / 60) 小时 \(minutes % 60) 分"
}

/// 数字加千分位
func grouped(_ n: Int64) -> String {
    n.formatted(.number.grouping(.automatic))
}

/// 库里存的时间是 UTC 的 ISO 字符串（`2026-10-16T08:01:02Z` 或 `2026-10-16 08:01:02`）
func relativeTime(_ stored: String) -> String {
    InterfaceLogic.formatRelative(stored)
}
