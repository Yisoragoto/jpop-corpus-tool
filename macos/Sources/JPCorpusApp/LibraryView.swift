import JPCorpusModel
import JPCore
import SwiftUI

/// 曲库：歌曲 / 专辑 / 歌手三种看法，点进去是专辑、歌手、歌曲的详情页。
/// 页面栈放在 AppModel 里，首页、⌘K 搜索、歌词面板都能直接推一页进来。
///
/// 不用 NavigationStack：它放在 NavigationSplitView 的正文栏里时，推出来的页会接管整个正文栏，
/// 挂在正文栏外面的歌词检查器、播放条、⌘K 面板都跟着消失（CI 截图里看到的）。自己按栈顶切换就没有这个问题
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
                    Button {
                        app.libraryPath.removeLast()
                    } label: {
                        Label("返回", systemImage: "chevron.left")
                    }
                    .keyboardShortcut("[", modifiers: [.command])
                    .help("返回（⌘[）")
                }
            }
        } else {
            LibraryRoot()
        }
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
            case .artists: ArtistGrid(filter: filter)
            case .graph: GraphView()
            case .timeline: ReleaseTimelineView()
            case .cloud: CloudView()
            }
        }
        .safeAreaInset(edge: .top) { NewSongsNotice().padding(.horizontal, 20) }
        .toolbar {
            ToolbarItem { LibraryManageButton() }
            ToolbarItem(placement: .principal) {
                Picker("", selection: Binding(get: { app.libraryMode },set: { app.libraryMode = $0 })) {
                    ForEach(LibraryMode.allCases, id: \.self) { Text($0.rawValue).tag($0) }
                }
                .pickerStyle(.segmented)
                .fixedSize()
            }
        }
        .searchable(text: $filter, placement: .toolbar, prompt: app.libraryMode == .artists ? "歌手名" : "曲名、歌手、专辑")
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

/// 右键菜单里的「收藏 / 取消收藏」：先问一下现在的状态。
/// 菜单项上的 .task 不一定跑得到（AppKit 菜单是另画的），没问到时标签两个都写
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
                              subtitle: [LibraryIndex.albumOwner(album.albumArtist), album.year].filter { !$0.isEmpty }.joined(separator: " · "),
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
        .task(id: "\(app.tracks.count)-\(app.library.version)") {
            do {
                albums = try await app.core?.albums() ?? []
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
        .task(id: app.library.version) {
            do {
                tracks = try await app.core?.albumTracks(album.id) ?? []
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
    @State private var memberships: [Membership] = []
    @State private var behind = false
    @State private var role: BehindRole = .all
    @State private var open: Set<Int64> = []
    private var directory: LibraryDirectory { LibraryIndex.buildDirectory(people, memberships) }
    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 14) {
                Picker("目录", selection: $behind) { Text("音乐人").tag(false); Text("幕后").tag(true) }.pickerStyle(.segmented)
                if behind {
                    Picker("角色", selection: $role) { ForEach(BehindRole.allCases, id: \.self) { Text($0 == .all ? "全部" : (roleLabels[$0.rawValue] ?? $0.rawValue)).tag($0) } }
                    ForEach(LibraryIndex.filterBehind(directory.behind, role, filter)) { person in
                        Button("\(person.name) · \(person.roleSummary)") { app.show(.person(person.id)) }.buttonStyle(.plain)
                    }
                } else {
                    ForEach(LibraryIndex.filterMusicians(directory.musicians, filter), id: \.row.entry.id) { hit in
                        HStack {
                            if !hit.row.members.isEmpty { Button { if open.contains(hit.row.entry.id) { open.remove(hit.row.entry.id) } else { open.insert(hit.row.entry.id) } } label: { Image(systemName: open.contains(hit.row.entry.id) || hit.memberHit ? "chevron.down" : "chevron.right") }.buttonStyle(.borderless) }
                            Button { app.show(.person(hit.row.entry.id)) } label: {
                                HStack { CoverImage(path: hit.row.entry.imagePath, size: 54, circle: true, placeholder: "person.fill", identity: hit.row.entry.name); VStack(alignment: .leading) { Text(hit.row.entry.name).font(.headline); Text(LibraryIndex.musicianSubtitle(hit.row)).font(.caption).foregroundStyle(.secondary) } }
                            }.buttonStyle(.plain)
                        }
                        if open.contains(hit.row.entry.id) || hit.memberHit {
                            ForEach(hit.row.members, id: \.membership.memberId) { member in
                                Button("\(member.membership.memberName) · \(LibraryIndex.memberSubtitle(member))") { app.show(.person(member.membership.memberId)) }.buttonStyle(.plain).padding(.leading, 76)
                            }
                        }
                    }
                }
            }.padding(20)
        }.clearScrollBackground()
            .task(id: "\(app.library.version)-\(app.tracks.count)") {
                do { people = try await app.core?.peopleDirectory() ?? []; memberships = try await app.library.client?.memberships() ?? [] }
                catch { app.errorMessage = error.localizedDescription }
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
    @State private var bands: [Membership] = []

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
                    CoverImage(path: person?.imagePath ?? "", size: 150, circle: true, placeholder: "person.fill", identity: person?.name ?? "")
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
                            Menu("可视化") {
                                Button("关系网") { app.visualizations.selected = personId; openVisualization(.graph) }
                                Button("时间树") { app.visualizations.treePerson = personId; app.visualizations.treeFocus = nil; openVisualization(.timeline) }
                                Button("词云") {
                                    if app.visualizations.cloudFilter.performerIds != [personId] {
                                        app.visualizations.cloudFilter.performerIds = [personId]; app.visualizations.picked = nil
                                    }
                                    openVisualization(.cloud)
                                }
                            }.menuStyle(.borderlessButton).fixedSize()
                        }
                        .padding(.top, 4)
                    }
                }

                if let person { PersonAdminView(personId: personId, name: person.name, person: person) }
                HStack {
                    SectionHeader(title: "作品", detail: "\(shown.count) 首")
                    if roles.count > 1 {
                        Picker("角色", selection: $role) { Text("全部").tag(""); ForEach(roles, id: \.self) { Text(roleLabels[$0] ?? $0).tag($0) } }.pickerStyle(.segmented).fixedSize()
                    }
                }
                let workSections = LibraryIndex.sectionWorks(personId, shown, bands)
                PersonReleaseAxis(sections: workSections,ended: person?.ended ?? "")
                ForEach(workSections) { section in PersonWorkSection(section: section) }

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
        .task(id: "\(personId)-\(app.library.version)") {
            guard let client = app.core else { return }
            do {
                async let dir = client.peopleDirectory()
                async let w = client.personWorks(personId)
                async let c = client.collaborators(personId)
                person = try await dir.first { $0.id == personId }
                works = try await w
                collaborators = try await c
                bands = try await app.library.client?.bandsOf(personId) ?? []
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
    private func openVisualization(_ mode: LibraryMode) { app.libraryMode = mode; app.libraryPath = [] }
}

@MainActor
private struct PersonWorkSection: View {
    let section: WorkSection
    @State private var expanded: Bool
    init(section: WorkSection) { self.section = section; _expanded = State(initialValue: !section.collapsed) }
    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            if !section.title.isEmpty { Button { expanded.toggle() } label: { Label(section.title, systemImage: expanded ? "chevron.down" : "chevron.right") }.buttonStyle(.plain).font(.headline) }
            if expanded {
                if !section.note.isEmpty { Text(section.note).font(.caption).foregroundStyle(.secondary) }
                TrackList(tracks: section.works.map(\.track), showArtist: true)
            }
        }
    }
}
