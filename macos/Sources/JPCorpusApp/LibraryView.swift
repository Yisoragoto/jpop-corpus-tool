import JPCorpusModel
import JPCore
import SwiftUI

// 自管页面栈，避免详情接管正文外的播放条与歌词检查器。
@MainActor
struct LibraryView: View {
    @Environment(AppModel.self) private var app

    var body: some View {
        if let route = app.libraryPath.last {
            Group {
                switch route {
                case .song(let id): SongDetailView(songId: id)
                case .album(let album): AlbumDetailView(album: album)
                case .person(let id): PersonDetailView(personId: id)
                }
            }
            .id(route)
            .toolbar {
                ToolbarItem(placement: .navigation) {
                    Button { app.libraryPath.removeLast() } label: {
                        Label("返回", systemImage: "chevron.left")
                    }
                    .keyboardShortcut("[", modifiers: [.command]).help("返回（⌘[）")
                }
            }
        } else { LibraryRoot() }
    }
}

@MainActor
private struct LibraryRoot: View {
    @Environment(AppModel.self) private var app
    @State private var filter = ""

    var body: some View {
        Group {
            switch app.libraryMode {
            case .songs: SongTable(filter: filter)
            case .albums: AlbumGrid(filter: filter)
            case .artists: LibraryArtistBrowser(filter: filter)
            case .graph: GraphView()
            case .timeline: ReleaseTimelineView()
            case .cloud: CloudView()
            }
        }
        .safeAreaInset(edge: .top, spacing: 0) {
            VStack(spacing: 10) {
                ViewThatFits(in: .horizontal) {
                    HStack(spacing: 16) { modePicker; Spacer(minLength: 0); searchField.frame(width: 220) }
                    VStack(alignment: .leading, spacing: 10) { modePicker; searchField }
                }
                NewSongsNotice()
            }.padding(.horizontal, 28).padding(.vertical, 10)
        }
        .toolbar { ToolbarItem { LibraryManageButton() } }
        .navigationTitle("资料库").navigationSubtitle("\(app.tracks.count) 首")
    }

    private var modePicker: some View {
        ScrollView(.horizontal) {
            HStack(spacing: 6) {
                ForEach(LibraryMode.allCases, id: \.self) { mode in
                    LibraryChip(title: mode == .artists ? "音乐人" : mode.rawValue, selected: app.libraryMode == mode) {
                        app.libraryMode = mode
                    }
                }
            }
        }.frame(height: 30).scrollIndicators(.never).accessibilityIdentifier("library-mode")
    }

    private var searchField: some View {
        HStack(spacing: 6) {
            Image(systemName: "magnifyingglass").foregroundStyle(.secondary)
            TextField(searchPlaceholder, text: $filter)
                .textFieldStyle(.plain).accessibilityIdentifier("library-search")
            if !filter.isEmpty {
                Button { filter = "" } label: { Image(systemName: "xmark.circle.fill").foregroundStyle(.secondary) }
                    .buttonStyle(.borderless).help("清除搜索")
            }
        }.padding(8).background(.quaternary, in: RoundedRectangle(cornerRadius: 6))
    }

    private var searchPlaceholder: String {
        switch app.libraryMode {
        case .artists: return "筛选名字"
        case .albums: return "筛选专辑 / 歌手"
        default: return "筛选歌名 / 歌手"
        }
    }
}

@MainActor
private struct SongTable: View {
    @Environment(AppModel.self) private var app
    let filter: String
    @State private var sort: LibrarySongSort = .title
    @State private var descending = false
    @State private var lyricsOnly = false

    private var rows: [Track] {
        let needle = filter.trimmingCharacters(in: .whitespacesAndNewlines)
        let base = app.tracks.filter { track in
            (!lyricsOnly || track.lineCount > 0) && (needle.isEmpty
                || track.title.localizedCaseInsensitiveContains(needle)
                || track.artist.localizedCaseInsensitiveContains(needle)
                || track.album.localizedCaseInsensitiveContains(needle))
        }
        return sort.sorted(base, descending: descending)
    }

    var body: some View {
        let songs = rows
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 24) {
                LibraryHero(title: "歌曲", kind: "资料库", subtitle: "\(app.tracks.count) 首",
                            imagePath: app.tracks.first(where: { !$0.coverPath.isEmpty })?.coverPath ?? "") {
                    LibraryPlayButton(disabled: songs.isEmpty) {
                        if let first = songs.first { Task { await app.play(songId: first.id, queue: songs) } }
                    }
                }
                HStack(spacing: 6) {
                    LibraryChip(title: "全部", selected: !lyricsOnly) { lyricsOnly = false }
                    LibraryChip(title: "有歌词", selected: lyricsOnly) { lyricsOnly = true }
                    Spacer()
                    Text("\(songs.count) 首").font(.caption).foregroundStyle(.secondary)
                }.padding(.horizontal, 28)
                LibrarySongSortChips(sort: $sort, descending: $descending).padding(.horizontal, 28)
                if songs.isEmpty {
                    ContentUnavailableView(filter.isEmpty && !lyricsOnly ? "曲库是空的，先去「导入」" : "没有匹配的歌", systemImage: "music.note")
                } else { TrackList(tracks: songs, showArtist: true).padding(.horizontal, 28) }
            }.padding(.bottom, 28)
        }.clearScrollBackground()
    }
}

@MainActor
struct FavoriteMenuItem: View {
    @Environment(AppModel.self) private var app
    let songId: String
    @State private var favorite: Bool?

    var body: some View {
        Button(favorite.map { $0 ? "取消收藏" : "收藏" } ?? "收藏 / 取消收藏") {
            Task {
                guard let client = app.core else { return }
                favorite = try? await client.toggleFavorite(song: songId)
                app.homeVersion += 1
            }
        }
        .task { favorite = try? await app.core?.isFavorite(song: songId) }
    }
}

private enum AlbumSort: String, CaseIterable {
    case newest = "最新在前", title = "专辑", artist = "歌手"
}

@MainActor
private struct AlbumGrid: View {
    @Environment(AppModel.self) private var app
    let filter: String
    @State private var albums: [Album] = []
    @State private var loading = true
    @State private var sort: AlbumSort = .newest

    private var rows: [Album] {
        let needle = filter.trimmingCharacters(in: .whitespacesAndNewlines)
        return albums.filter {
            needle.isEmpty || $0.title.localizedCaseInsensitiveContains(needle) || $0.albumArtist.localizedCaseInsensitiveContains(needle)
        }.sorted { a, b in
            if sort == .newest && a.year != b.year { return a.year > b.year }
            if sort == .artist && a.albumArtist != b.albumArtist {
                return a.albumArtist.compare(b.albumArtist, locale: Locale(identifier: "ja")) == .orderedAscending
            }
            let order = a.title.compare(b.title, locale: Locale(identifier: "ja"))
            return order == .orderedSame ? a.id < b.id : order == .orderedAscending
        }
    }

    var body: some View {
        let items = rows
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 28) {
                LibraryHero(title: "专辑", kind: "资料库", subtitle: "\(albums.count) 张专辑",
                            imagePath: items.first?.artworkPath ?? "", placeholder: "square.stack") { EmptyView() }
                Group {
                    if loading { ProgressView("加载中…").frame(maxWidth: .infinity) }
                    else if items.isEmpty {
                        ContentUnavailableView(filter.isEmpty ? "还没有专辑信息" : "没有匹配的专辑", systemImage: "square.stack")
                    } else {
                        if filter.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty { newestShelf }
                        HStack(spacing: 6) {
                            ForEach(AlbumSort.allCases, id: \.self) { order in
                                LibraryChip(title: order.rawValue, selected: sort == order) { sort = order }
                            }
                            Spacer()
                            Text("\(items.count) 张").font(.caption).foregroundStyle(.secondary)
                        }
                        LazyVGrid(columns: LibraryAppearance.columns, alignment: .leading, spacing: 22) {
                            ForEach(items) { album in albumTile(album) }
                        }
                    }
                }.padding(.horizontal, 28)
            }.padding(.bottom, 28)
        }
        .clearScrollBackground()
        .task(id: "\(app.tracks.count)-\(app.library.version)") {
            loading = true
            defer { loading = false }
            do { albums = try await app.core?.albums() ?? [] }
            catch { app.errorMessage = error.localizedDescription }
        }
    }

    private var newestShelf: some View {
        VStack(alignment: .leading, spacing: 12) {
            SectionHeader(title: "专辑", detail: "最新在前")
            ScrollView(.horizontal) {
                LazyHStack(alignment: .top, spacing: 14) {
                    ForEach(Array(albums.sorted { $0.year == $1.year ? $0.id < $1.id : $0.year > $1.year }.prefix(12))) { album in
                        albumTile(album, size: 148)
                    }
                }.padding(.vertical, 6)
            }.frame(height: 200).scrollIndicators(.never)
        }
    }

    private func albumTile(_ album: Album, size: CGFloat = 140) -> some View {
        CoverTile(coverPath: album.artworkPath, title: album.title.isEmpty ? "（无题）" : album.title,
                  subtitle: [LibraryIndex.albumOwner(album.albumArtist), String(album.year.prefix(4)), "\(album.trackCount) 首"]
                    .filter { !$0.isEmpty }.joined(separator: " · "), size: size, placeholder: "square.stack") {
            app.show(.album(album))
        }
    }
}

@MainActor
struct AlbumDetailView: View {
    @Environment(AppModel.self) private var app
    let album: Album
    @State private var tracks: [Track] = []
    @State private var loading = true

    var body: some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 24) {
                LibraryHero(title: album.title.isEmpty ? "（无题）" : album.title, kind: "专辑", subtitle: subtitle,
                            imagePath: album.artworkPath, placeholder: "square.stack") {
                    LibraryPlayButton(disabled: tracks.isEmpty) {
                        let queue = tracks
                        if let first = queue.first { Task { await app.play(songId: first.id, queue: queue) } }
                    }
                }
                Group {
                    if loading { ProgressView("加载中…").frame(maxWidth: .infinity) }
                    else if tracks.isEmpty { ContentUnavailableView("没有作品记录", systemImage: "music.note") }
                    else { TrackList(tracks: tracks, showArtist: true) }
                }.padding(.horizontal, 28)
            }.padding(.bottom, 28)
        }
        .clearScrollBackground().navigationTitle(album.title)
        .task(id: "\(album.id)-\(app.library.version)") {
            loading = true
            defer { loading = false }
            do { tracks = try await app.core?.albumTracks(album.id) ?? [] }
            catch { app.errorMessage = error.localizedDescription }
        }
    }

    private var subtitle: String {
        [album.albumArtist, String(album.year.prefix(4)), "\(album.trackCount) 首", formatListening(album.totalDurationSec)]
            .filter { !$0.isEmpty }.joined(separator: " · ")
    }
}
