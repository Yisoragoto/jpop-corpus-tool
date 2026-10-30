import AppKit
import JPCore
import JPCorpusModel
import SwiftUI

// Windows 的封面墙间距与角色颜色在曲库内共用。
enum LibraryAppearance {
    static let columns = [GridItem(.adaptive(minimum: 140, maximum: 180), spacing: 18)]

    static func roleColor(_ role: String) -> Color {
        switch role {
        case "composer": return Color(red: 0.97, green: 0.66, blue: 0.77)
        case "lyricist": return Color(red: 1, green: 0.83, blue: 0.47)
        case "arranger": return Color(red: 0.61, green: 0.89, blue: 0.65)
        default: return Color(red: 0.30, green: 0.76, blue: 1)
        }
    }
}

struct LibraryChip: View {
    let title: String
    let selected: Bool
    var dot: Color?
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            HStack(spacing: 6) {
                if let dot { Circle().fill(dot).frame(width: 7, height: 7) }
                Text(title).font(.system(size: 12, weight: selected ? .semibold : .regular))
            }
            .padding(.horizontal, 10).padding(.vertical, 6)
            .foregroundStyle(selected ? Color.accentColor : Color.primary.opacity(0.75))
            .background(selected ? Color.accentColor.opacity(0.14) : Color.primary.opacity(0.045),
                        in: RoundedRectangle(cornerRadius: 6))
            .contentShape(RoundedRectangle(cornerRadius: 6))
        }
        .buttonStyle(.plain)
        .accessibilityAddTraits(selected ? [.isSelected] : [])
    }
}

@MainActor
struct LibraryArtworkBackdrop: View {
    @Environment(AppModel.self) private var app
    let path: String
    @State private var image: NSImage?

    var body: some View {
        GeometryReader { proxy in
            if let image {
                Image(nsImage: image).resizable().aspectRatio(contentMode: .fill)
                    .frame(width: proxy.size.width, height: proxy.size.height)
                    .opacity(0.24)
            }
        }
        .clipped().allowsHitTesting(false).accessibilityHidden(true)
        .task(id: "\(path)-\(app.library.version)") {
            image = nil
            guard !path.isEmpty, let client = app.client else { return }
            let resolved = (try? await client.resolveMedia(path)) ?? nil
            // 复用后台虚化缓存，避免播放刷新时反复做整块模糊。
            let decoded = await Artwork.blurred(atPath: resolved, radius: 48, saturation: 1.3, displayWidth: 800)
            guard !Task.isCancelled else { return }
            image = decoded
        }
    }
}

@MainActor
struct LibraryHero<Actions: View>: View {
    let title: String
    let kind: String
    let subtitle: String
    let imagePath: String
    var circle = false
    var placeholder = "music.note"
    @ViewBuilder var actions: () -> Actions

    var body: some View {
        ViewThatFits(in: .horizontal) {
            HStack(alignment: .bottom, spacing: 24) {
                artwork
                information.frame(minWidth: 240, maxWidth: .infinity, alignment: .leading)
            }
            VStack(alignment: .leading, spacing: 18) { artwork; information }
        }
        .padding(.horizontal, 28).padding(.vertical, 28)
        .frame(maxWidth: .infinity, minHeight: 220, alignment: .bottomLeading)
        .background { LibraryArtworkBackdrop(path: imagePath) }
        .background(LinearGradient(colors: [Color.accentColor.opacity(0.08), .clear], startPoint: .top, endPoint: .bottom))
    }

    private var artwork: some View {
        CoverImage(path: imagePath, size: 104, cornerRadius: 10, circle: circle,
                   placeholder: placeholder, identity: title)
            .shadow(color: .black.opacity(0.24), radius: 16, y: 6)
    }

    private var information: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text(kind).font(.system(size: 12, weight: .semibold)).foregroundStyle(.secondary)
            Text(title).font(.system(size: 28, weight: .bold)).lineLimit(2)
                .fixedSize(horizontal: false, vertical: true).textSelection(.enabled)
            Text(subtitle).font(.callout).foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
            actions()
        }
    }
}

struct LibraryPlayButton: View {
    var disabled = false
    let action: () -> Void

    var body: some View {
        Button(action: action) { Label("播放", systemImage: "play.fill").padding(.horizontal, 10) }
            .buttonStyle(.borderedProminent).buttonBorderShape(.capsule)
            .controlSize(.large).disabled(disabled)
    }
}

enum LibrarySongSort: String, CaseIterable {
    case title = "歌名", artist = "歌手", album = "专辑", year = "年份", duration = "时长"

    func sorted(_ tracks: [Track], descending: Bool) -> [Track] {
        tracks.sorted { a, b in
            let order: ComparisonResult
            switch self {
            case .title: order = compare(a.title, b.title)
            case .artist: order = compare(a.artist, b.artist)
            case .album: order = compare(a.album, b.album)
            case .year: order = compare(a.year, b.year)
            case .duration:
                let left = a.durationSec ?? 0, right = b.durationSec ?? 0
                order = left == right ? .orderedSame : (left < right ? .orderedAscending : .orderedDescending)
            }
            let tied = order == .orderedSame ? compare(a.title, b.title) : order
            if tied == .orderedSame { return a.id < b.id }
            return descending ? tied == .orderedDescending : tied == .orderedAscending
        }
    }

    private func compare(_ a: String, _ b: String) -> ComparisonResult {
        a.compare(b, options: [.caseInsensitive, .numeric], locale: Locale(identifier: "ja"))
    }
}

struct LibrarySongSortChips: View {
    @Binding var sort: LibrarySongSort
    @Binding var descending: Bool

    var body: some View {
        ScrollView(.horizontal) {
            HStack(spacing: 6) {
                Text("排序").font(.caption).foregroundStyle(.secondary).padding(.trailing, 4)
                ForEach(LibrarySongSort.allCases, id: \.self) { value in
                    LibraryChip(title: value.rawValue, selected: sort == value) { sort = value }
                }
                Button { descending.toggle() } label: {
                    Image(systemName: descending ? "arrow.down" : "arrow.up").frame(width: 26, height: 26)
                }
                .buttonStyle(.plain).help(descending ? "降序" : "升序")
                .accessibilityLabel(descending ? "降序" : "升序")
            }
        }.frame(height: 30).scrollIndicators(.never)
    }
}

private struct LibraryTrackEntry: Identifiable {
    let number: Int
    let track: Track
    var id: String { track.id }
}

/// 只有进入可见区域的歌曲行才会创建封面任务。
@MainActor
struct TrackList: View {
    let tracks: [Track]
    var showArtist = false
    var trailing: ((Track) -> String)?
    var queue: [Track]?
    @State private var wide = false

    private var entries: [LibraryTrackEntry] {
        tracks.enumerated().map { LibraryTrackEntry(number: $0.offset + 1, track: $0.element) }
    }

    var body: some View {
        LazyVStack(spacing: 2) {
            columnHeadings
            ForEach(entries) { entry in
                LibrarySongRow(track: entry.track, number: entry.number, showArtist: showArtist,
                               wide: wide, annotation: trailing?(entry.track), queue: queue ?? tracks)
            }
        }
        .background {
            GeometryReader { proxy in
                Color.clear.onAppear { wide = proxy.size.width >= 700 }
                    .onChange(of: proxy.size.width) { _, width in wide = width >= 700 }
            }
        }
    }

    private var columnHeadings: some View {
        HStack(spacing: 12) {
            Text("#").frame(width: 24, alignment: .trailing)
            Text("歌曲").frame(maxWidth: .infinity, alignment: .leading).padding(.leading, 52)
            if wide {
                Text("专辑").frame(width: 160, alignment: .leading)
                Text("年份").frame(width: 54, alignment: .leading)
            }
            Text("时长").frame(width: 52, alignment: .trailing)
        }
        .font(.caption).foregroundStyle(.secondary)
        .padding(.horizontal, 12).padding(.vertical, 10)
        .overlay(alignment: .bottom) { Divider() }
        .accessibilityHidden(true)
    }
}

@MainActor
private struct LibrarySongRow: View {
    @Environment(AppModel.self) private var app
    let track: Track
    let number: Int
    let showArtist: Bool
    let wide: Bool
    let annotation: String?
    let queue: [Track]
    @State private var hovering = false
    @FocusState private var playFocused: Bool
    private var current: Bool { app.player.track?.id == track.id }

    var body: some View {
        HStack(spacing: 12) {
            Text(current ? "♪" : "\(number)")
                .font(.callout.monospacedDigit()).foregroundStyle(current ? Color.accentColor : Color.secondary)
                .frame(width: 24, alignment: .trailing)
            playCover
            Button { app.show(.song(track.id)) } label: { metadata }
                .buttonStyle(.plain)
        }
        .padding(.horizontal, 12).padding(.vertical, 8)
        .background(current ? Color.accentColor.opacity(0.12) : Color.primary.opacity(hovering ? 0.055 : 0),
                    in: RoundedRectangle(cornerRadius: 6))
        .contentShape(Rectangle()).onHover { hovering = $0 }
        .contextMenu {
            Button("播放") { play() }
            Button("歌曲详情") { app.show(.song(track.id)) }
            FavoriteMenuItem(songId: track.id)
        }
    }

    private var playCover: some View {
        Button { play() } label: {
            CoverImage(path: track.coverPath, size: 40, cornerRadius: 6)
                .overlay {
                    ZStack {
                        RoundedRectangle(cornerRadius: 6).fill(.black.opacity(0.55))
                        Image(systemName: "play.fill").font(.system(size: 15)).foregroundStyle(.white)
                    }
                    .opacity(hovering || playFocused || current ? 1 : 0)
                }
        }
        .buttonStyle(.plain).focused($playFocused)
        .help("播放「\(track.title)」").accessibilityLabel("播放「\(track.title)」")
    }

    private var metadata: some View {
        HStack(spacing: 12) {
            VStack(alignment: .leading, spacing: 3) {
                Text(track.title).font(.system(size: 14, weight: .medium))
                    .foregroundStyle(current ? Color.accentColor : Color.primary).lineLimit(1)
                if showArtist { Text(track.artist).font(.caption).foregroundStyle(.secondary).lineLimit(1) }
                if !wide {
                    Text([track.album, String(track.year.prefix(4))].filter { !$0.isEmpty }.joined(separator: " · "))
                        .font(.caption).foregroundStyle(.secondary).lineLimit(1)
                }
                if track.lineCount == 0 { Text("无歌词").font(.caption2).foregroundStyle(.orange) }
                if let annotation, !annotation.isEmpty {
                    Text(annotation).font(.caption2).foregroundStyle(.secondary).lineLimit(1)
                }
            }.frame(maxWidth: .infinity, alignment: .leading)
            if wide {
                Text(track.album.isEmpty ? "—" : track.album).font(.caption).foregroundStyle(.secondary)
                    .lineLimit(1).frame(width: 160, alignment: .leading)
                Text(track.year.isEmpty ? "—" : String(track.year.prefix(4)))
                    .font(.caption.monospacedDigit()).foregroundStyle(.secondary).frame(width: 54, alignment: .leading)
            }
            Text(track.durationSec.map(formatTime) ?? "—").font(.callout.monospacedDigit())
                .foregroundStyle(.secondary).frame(width: 52, alignment: .trailing)
        }
        .contentShape(Rectangle())
        .help("\(track.title) · \(track.artist) · \(track.album)")
    }

    private func play() {
        Task { await app.play(songId: track.id, queue: queue) }
    }
}
