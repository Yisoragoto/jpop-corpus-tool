import JPCore
import SwiftUI

/// 曲库：歌曲 / 专辑 / 歌手三种看法，点进去是专辑、歌手、歌曲的详情页。
/// 导航栈放在 AppModel 里，首页、⌘K 搜索、歌词面板都能直接推一页进来
@MainActor
struct LibraryView: View {
    @Environment(AppModel.self) private var app

    var body: some View {
        @Bindable var app = app
        NavigationStack(path: $app.libraryPath) {
            LibraryRoot()
                .navigationDestination(for: LibraryRoute.self) { route in
                    switch route {
                    case .song(let id): SongDetailView(songId: id)
                    case .album(let album): AlbumDetailView(album: album)
                    case .person(let id): PersonDetailView(personId: id)
                    }
                }
        }
    }
}

private enum LibraryMode: String, CaseIterable {
    case songs = "歌曲", albums = "专辑", artists = "歌手"
}

@MainActor
private struct LibraryRoot: View {
    @Environment(AppModel.self) private var app
    @AppStorage("libraryMode") private var mode: LibraryMode = .songs
    @State private var filter = ""

    var body: some View {
        Group {
            switch mode {
            case .songs: SongTable(filter: filter)
            case .albums: AlbumGrid(filter: filter)
            case .artists: ArtistGrid(filter: filter)
            }
        }
        .toolbar {
            ToolbarItem(placement: .principal) {
                Picker("", selection: $mode) {
                    ForEach(LibraryMode.allCases, id: \.self) { Text($0.rawValue).tag($0) }
                }
                .pickerStyle(.segmented)
                .fixedSize()
            }
        }
        .searchable(text: $filter, placement: .toolbar, prompt: mode == .artists ? "歌手名" : "曲名、歌手、专辑")
        .navigationTitle("曲库")
        .navigationSubtitle("\(app.tracks.count) 首")
    }
}

// MARK: - 歌曲

@MainActor
private struct SongTable: View {
    @Environment(AppModel.self) private var app
    let filter: String
    @State private var selection: Set<Track.ID> = []
    @State private var sortOrder = [KeyPathComparator(\Track.artist), KeyPathComparator(\Track.title)]

    private var rows: [Track] {
        let needle = filter.trimmingCharacters(in: .whitespaces)
        let base = needle.isEmpty ? app.tracks : app.tracks.filter {
            $0.title.localizedCaseInsensitiveContains(needle)
                || $0.artist.localizedCaseInsensitiveContains(needle)
                || $0.album.localizedCaseInsensitiveContains(needle)
        }
        return base.sorted(using: sortOrder)
    }

    var body: some View {
        Table(rows, selection: $selection, sortOrder: $sortOrder) {
            TableColumn("曲名", value: \.title)
            TableColumn("歌手", value: \.artist)
            TableColumn("专辑", value: \.album)
            TableColumn("年份", value: \.year).width(56)
            TableColumn("歌词") { t in
                Text(t.lineCount > 0 ? "\(t.lineCount) 行" : "无").foregroundStyle(.secondary)
            }
            .width(56)
            TableColumn("时长") { t in
                Text(t.durationSec.map(formatTime) ?? "—").monospacedDigit().foregroundStyle(.secondary)
            }
            .width(52)
        }
        .clearScrollBackground()
        .alternatingRowBackgrounds(.disabled)
        .contextMenu(forSelectionType: Track.ID.self) { ids in
            if let id = ids.first {
                Button("播放") { Task { await app.play(songId: id) } }
                Button("歌曲详情") { app.show(.song(id)) }
                FavoriteMenuItem(songId: id)
            }
        } primaryAction: { ids in
            if let id = ids.first { Task { await app.play(songId: id) } }
        }
    }
}

/// 右键菜单里的「收藏 / 取消收藏」：先问一下现在的状态
@MainActor
struct FavoriteMenuItem: View {
    @Environment(AppModel.self) private var app
    let songId: String
    @State private var favorite: Bool?

    var body: some View {
        Button(favorite == true ? "取消收藏" : "收藏") {
            Task {
                guard let client = app.client else { return }
                favorite = try? await client.toggleFavorite(song: songId)
                app.homeVersion += 1
            }
        }
        .task { favorite = try? await app.client?.isFavorite(song: songId) }
    }
}

// MARK: - 专辑

@MainActor
private struct AlbumGrid: View {
    @Environment(AppModel.self) private var app
    let filter: String
    @State private var albums: [Album] = []

    private var rows: [Album] {
        let needle = filter.trimmingCharacters(in: .whitespaces)
        guard !needle.isEmpty else { return albums }
        return albums.filter { $0.title.localizedCaseInsensitiveContains(needle) || $0.albumArtist.localizedCaseInsensitiveContains(needle) }
    }

    var body: some View {
        ScrollView {
            LazyVGrid(columns: [GridItem(.adaptive(minimum: 150, maximum: 180), spacing: 18)], alignment: .leading, spacing: 22) {
                ForEach(rows) { album in
                    CoverTile(coverPath: album.artworkPath, title: album.title,
                              subtitle: [album.albumArtist, album.year].filter { !$0.isEmpty }.joined(separator: " · "),
                              size: 150, placeholder: "square.stack") {
                        app.libraryPath.append(.album(album))
                    }
                }
            }
            .padding(20)
        }
        .clearScrollBackground()
        .overlay {
            if albums.isEmpty { ContentUnavailableView("还没有专辑", systemImage: "square.stack") }
        }
        .task(id: app.tracks.count) {
            do {
                albums = try await app.client?.albums() ?? []
            } catch {
                app.errorMessage = error.localizedDescription
            }
        }
    }
}

@MainActor
struct AlbumDetailView: View {
    @Environment(AppModel.self) private var app
    let album: Album
    @State private var tracks: [Track] = []

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                HStack(alignment: .bottom, spacing: 20) {
                    CoverImage(path: album.artworkPath, size: 180, cornerRadius: 12, placeholder: "square.stack")
                        .shadow(color: .black.opacity(0.22), radius: 14, y: 6)
                    VStack(alignment: .leading, spacing: 6) {
                        Text("专辑").font(.caption.weight(.semibold)).foregroundStyle(.secondary)
                        Text(album.title).font(.largeTitle.weight(.bold)).lineLimit(2)
                        Text([album.albumArtist, album.year, "\(album.trackCount) 首", formatListening(album.totalDurationSec)]
                            .filter { !$0.isEmpty }.joined(separator: " · "))
                            .foregroundStyle(.secondary)
                        Button {
                            if let first = tracks.first { Task { await app.play(songId: first.id) } }
                        } label: {
                            Label("播放", systemImage: "play.fill")
                        }
                        .buttonStyle(.borderedProminent)
                        .disabled(tracks.isEmpty)
                        .padding(.top, 6)
                    }
                }
                TrackList(tracks: tracks, showArtist: true)
            }
            .padding(24)
        }
        .clearScrollBackground()
        .navigationTitle(album.title)
        .task {
            do {
                tracks = try await app.client?.albumTracks(album.id) ?? []
            } catch {
                app.errorMessage = error.localizedDescription
            }
        }
    }
}

/// 详情页里的曲目列表：双击放，右键看详情 / 收藏
@MainActor
struct TrackList: View {
    @Environment(AppModel.self) private var app
    let tracks: [Track]
    var showArtist = false
    var trailing: ((Track) -> String)?

    var body: some View {
        VStack(spacing: 0) {
            ForEach(Array(tracks.enumerated()), id: \.element.id) { index, t in
                HStack(spacing: 12) {
                    Text("\(index + 1)").font(.callout.monospacedDigit()).foregroundStyle(.tertiary).frame(width: 24, alignment: .trailing)
                    VStack(alignment: .leading, spacing: 1) {
                        Text(t.title).lineLimit(1)
                        if showArtist {
                            Text(t.artist).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                        }
                    }
                    Spacer()
                    if let trailing {
                        Text(trailing(t)).font(.caption).foregroundStyle(.secondary)
                    }
                    if t.lineCount == 0 {
                        Text("无歌词").font(.caption).foregroundStyle(.orange)
                    }
                    Text(t.durationSec.map(formatTime) ?? "—").font(.callout.monospacedDigit()).foregroundStyle(.secondary)
                }
                .padding(.horizontal, 12)
                .padding(.vertical, 8)
                .background(app.player.track?.id == t.id ? AnyShapeStyle(.tint.opacity(0.15)) : AnyShapeStyle(.clear))
                .contentShape(Rectangle())
                .onTapGesture(count: 2) { Task { await app.play(songId: t.id) } }
                .contextMenu {
                    Button("播放") { Task { await app.play(songId: t.id) } }
                    Button("歌曲详情") { app.show(.song(t.id)) }
                    FavoriteMenuItem(songId: t.id)
                }
                if t.id != tracks.last?.id { Divider().padding(.leading, 48) }
            }
        }
        .glassCard(cornerRadius: 14)
    }
}

// MARK: - 歌手

@MainActor
private struct ArtistGrid: View {
    @Environment(AppModel.self) private var app
    let filter: String
    @State private var people: [DirectoryEntry] = []
    @AppStorage("artistsPerformersOnly") private var performersOnly = true

    private var rows: [DirectoryEntry] {
        let needle = filter.trimmingCharacters(in: .whitespaces)
        return people.filter { p in
            (!performersOnly || p.performer > 0) && (needle.isEmpty || p.name.localizedCaseInsensitiveContains(needle))
        }
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 12) {
                Toggle("只看演唱者（不含只作词作曲的人）", isOn: $performersOnly)
                    .toggleStyle(.checkbox)
                    .font(.callout)
                LazyVGrid(columns: [GridItem(.adaptive(minimum: 130, maximum: 150), spacing: 18)], spacing: 22) {
                    ForEach(rows) { p in
                        CoverTile(coverPath: p.imagePath, title: p.name, subtitle: p.roleSummary, size: 120, circle: true, placeholder: "person.fill") {
                            app.libraryPath.append(.person(p.id))
                        }
                    }
                }
            }
            .padding(20)
        }
        .clearScrollBackground()
        .task(id: app.tracks.count) {
            do {
                people = try await app.client?.peopleDirectory() ?? []
            } catch {
                app.errorMessage = error.localizedDescription
            }
        }
    }
}

@MainActor
struct PersonDetailView: View {
    @Environment(AppModel.self) private var app
    let personId: Int64
    @State private var person: DirectoryEntry?
    @State private var works: [PersonWork] = []
    @State private var collaborators: [Collaborator] = []
    @State private var role = ""

    private var shown: [PersonWork] {
        role.isEmpty ? works : works.filter { $0.roles.contains(role) }
    }

    private var roles: [String] {
        ["performer", "composer", "lyricist", "arranger"].filter { r in works.contains { $0.roles.contains(r) } }
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 22) {
                HStack(alignment: .center, spacing: 22) {
                    CoverImage(path: person?.imagePath ?? "", size: 150, circle: true, placeholder: "person.fill")
                        .shadow(color: .black.opacity(0.2), radius: 12, y: 5)
                    VStack(alignment: .leading, spacing: 6) {
                        Text(person?.artistType == "Group" ? "组合" : "歌手").font(.caption.weight(.semibold)).foregroundStyle(.secondary)
                        Text(person?.name ?? "").font(.largeTitle.weight(.bold))
                        if let p = person {
                            Text([p.roleSummary, years(p)].filter { !$0.isEmpty }.joined(separator: " · "))
                                .foregroundStyle(.secondary)
                        }
                        HStack {
                            Button {
                                if let first = shown.first(where: { $0.track.lineCount > 0 }) ?? shown.first {
                                    Task { await app.play(songId: first.track.id) }
                                }
                            } label: {
                                Label("播放", systemImage: "play.fill")
                            }
                            .buttonStyle(.borderedProminent)
                            .disabled(works.isEmpty)
                            Button("分析这个人的歌") {
                                app.analysisScope = ScopeRequest(name: person?.name ?? "", performerIds: [personId])
                                app.section = .analysis
                            }
                            .help("词频和子语料对比都限定在这位歌手演唱的歌里")
                        }
                        .padding(.top, 4)
                    }
                }

                VStack(alignment: .leading, spacing: 10) {
                    HStack {
                        SectionHeader(title: "作品", detail: "\(shown.count) 首")
                        if roles.count > 1 {
                            Picker("", selection: $role) {
                                Text("全部").tag("")
                                ForEach(roles, id: \.self) { Text(roleLabels[$0] ?? $0).tag($0) }
                            }
                            .pickerStyle(.segmented)
                            .fixedSize()
                        }
                    }
                    TrackList(tracks: shown.map(\.track), showArtist: true) { t in
                        let r = works.first { $0.track.id == t.id }?.roles ?? []
                        return ([t.year] + r.filter { $0 != "performer" }.map { roleLabels[$0] ?? $0 }).filter { !$0.isEmpty }.joined(separator: " · ")
                    }
                }

                if !collaborators.isEmpty {
                    VStack(alignment: .leading, spacing: 10) {
                        SectionHeader(title: "合作过的人", detail: nil)
                        ScrollView(.horizontal) {
                            LazyHStack(alignment: .top, spacing: 16) {
                                ForEach(collaborators) { c in
                                    CoverTile(coverPath: c.imagePath, title: c.name,
                                              subtitle: "\(c.sharedTracks) 首 · \(c.roles.map { roleLabels[$0] ?? $0 }.joined(separator: "/"))",
                                              size: 84, circle: true, placeholder: "person.fill") {
                                        app.libraryPath.append(.person(c.personId))
                                    }
                                }
                            }
                            .padding(.vertical, 6)
                        }
                        .scrollIndicators(.never)
                    }
                }
            }
            .padding(24)
        }
        .clearScrollBackground()
        .navigationTitle(person?.name ?? "歌手")
        .task(id: personId) {
            guard let client = app.client else { return }
            do {
                async let dir = client.peopleDirectory()
                async let w = client.personWorks(personId)
                async let c = client.collaborators(personId)
                person = try await dir.first { $0.id == personId }
                works = try await w
                collaborators = try await c
            } catch {
                app.errorMessage = error.localizedDescription
            }
        }
    }

    private func years(_ p: DirectoryEntry) -> String {
        switch (p.firstYear.isEmpty, p.lastYear.isEmpty) {
        case (false, false): return p.firstYear == p.lastYear ? p.firstYear : "\(p.firstYear)–\(p.lastYear)"
        case (false, true): return p.firstYear
        default: return ""
        }
    }
}
