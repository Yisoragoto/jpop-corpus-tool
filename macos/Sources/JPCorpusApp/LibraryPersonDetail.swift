import JPCore
import JPCorpusModel
import SwiftUI

private enum LibraryPersonTab: String, CaseIterable {
    case overview = "概览", works = "作品", people = "成员与合作", timeline = "时间轴"
}

@MainActor
struct PersonDetailView: View {
    @Environment(AppModel.self) private var app
    let personId: Int64
    @State private var person: DirectoryEntry?
    @State private var fallback: (name: String, imagePath: String)?
    @State private var works: [PersonWork] = []
    @State private var collaborators: [Collaborator] = []
    @State private var albums: [Album] = []
    @State private var memberships: [Membership] = []
    @State private var role = ""
    @State private var tab: LibraryPersonTab = .overview
    @State private var loading = true
    @AppStorage("jp.profile.singlesCovers") private var covers = false

    private var displayName: String { person?.name ?? fallback?.name ?? "" }
    private var imagePath: String { person?.imagePath ?? fallback?.imagePath ?? "" }
    private var bands: [Membership] { memberships.filter { $0.memberId == personId } }
    private var members: [Membership] { memberships.filter { $0.bandId == personId } }
    private var shown: [PersonWork] { role.isEmpty ? works : works.filter { $0.roles.contains(role) } }
    private var roles: [String] {
        ["performer", "composer", "lyricist", "arranger"].filter { r in works.contains { $0.roles.contains(r) } }
    }

    var body: some View {
        let sections = LibraryIndex.sectionWorks(personId, shown, bands)
        let queue = sections.flatMap { $0.works.map(\.track) }
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 24) {
                LibraryArtistHero(name: displayName, imagePath: imagePath, person: person, isBand: !members.isEmpty || person?.artistType == "Group") {
                    heroActions(queue: queue)
                }
                tabs
                if loading {
                    ProgressView("加载作品…").frame(maxWidth: .infinity, minHeight: 120)
                } else {
                    switch tab {
                    case .overview: overview(sections: sections)
                    case .works: worksContent(sections: sections, queue: queue)
                    case .people: peopleContent
                    case .timeline:
                        VStack(alignment: .leading, spacing: 16) {
                            PersonReleaseAxis(sections: sections, ended: person?.ended ?? "")
                            Button("在时间树里看") {
                                app.visualizations.treePerson = personId
                                app.visualizations.treeFocus = nil
                                openVisualization(.timeline)
                            }
                        }.padding(.horizontal, 28)
                    }
                }
            }.padding(.bottom, 28)
        }
        .clearScrollBackground().navigationTitle(displayName.isEmpty ? "音乐人" : displayName)
        .onChange(of: tab) { _, value in if value != .works { role = "" } }
        .task(id: "\(personId)-\(app.library.version)") { await load() }
    }

    private var tabs: some View {
        ScrollView(.horizontal) {
            HStack(spacing: 28) {
                ForEach(LibraryPersonTab.allCases, id: \.self) { value in
                    Button { tab = value } label: {
                        Text(value.rawValue).font(.system(size: 14, weight: tab == value ? .semibold : .regular))
                            .foregroundStyle(tab == value ? Color.primary : Color.secondary)
                            .padding(.vertical, 13)
                            .overlay(alignment: .bottom) {
                                if tab == value { Capsule().fill(Color.accentColor).frame(height: 3) }
                            }
                    }
                    .buttonStyle(.plain).accessibilityAddTraits(tab == value ? [.isSelected] : [])
                }
            }.padding(.horizontal, 28)
        }.frame(height: 44).scrollIndicators(.never).overlay(alignment: .bottom) { Divider() }
    }

    private func heroActions(queue: [Track]) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            if !bands.isEmpty {
                ScrollView(.horizontal) {
                    LazyHStack(spacing: 8) {
                        ForEach(bands, id: \.bandId) { band in
                            Button { app.show(.person(band.bandId)) } label: {
                                HStack(spacing: 7) {
                                    CoverImage(path: band.bandImage, size: 20, circle: true, placeholder: "person.fill", identity: band.bandName)
                                    Text(band.leftAt.isEmpty ? "\(band.bandName) 成员" : "曾属 \(band.bandName)")
                                }.font(.caption).padding(4).padding(.trailing, 6).background(.ultraThinMaterial, in: Capsule())
                            }.buttonStyle(.plain)
                        }
                    }
                }.frame(height: 30).scrollIndicators(.never)
            }
            ScrollView(.horizontal) {
                HStack(spacing: 10) {
                    LibraryPlayButton(disabled: queue.isEmpty) {
                        if let first = queue.first(where: { $0.lineCount > 0 }) ?? queue.first {
                            Task { await app.play(songId: first.id, queue: queue) }
                        }
                    }
                    Button("分析这个人的歌") {
                        app.analysisScope = ScopeRequest(name: displayName, performerIds: [personId])
                        app.section = .analysis
                    }
                    Menu("可视化") {
                        Button("关系网") { app.visualizations.selected = personId; openVisualization(.graph) }
                        Button("时间轴") {
                            app.visualizations.treePerson = personId; app.visualizations.treeFocus = nil
                            openVisualization(.timeline)
                        }
                        Button("词云") {
                            if app.visualizations.cloudFilter.performerIds != [personId] {
                                app.visualizations.cloudFilter.performerIds = [personId]; app.visualizations.picked = nil
                            }
                            openVisualization(.cloud)
                        }
                    }.menuStyle(.borderlessButton).fixedSize()
                }
            }.frame(height: 40).scrollIndicators(.never)
        }
    }

    @ViewBuilder
    private func overview(sections: [WorkSection]) -> some View {
        let main = sections.first(where: { !$0.collapsed })?.works.map(\.track) ?? []
        let groups = PersonReleases.groupByAlbum(main)
        let types = Dictionary(uniqueKeysWithValues: albums.map { ($0.id, $0.releaseType ?? "") })
        let releases = PersonReleases.splitReleases(groups, types: types)
        let records = (releases.albums + releases.singles.filter { $0.kind == "ep" }).sorted { $0.year > $1.year }
        let songs = PersonReleases.allSongs(groups)
        if songs.isEmpty {
            ContentUnavailableView("没有作品记录", systemImage: "music.note")
        } else {
            HStack(spacing: 10) {
                SectionHeader(title: "单曲", detail: "\(songs.count) 首 · 最新在前")
                LibraryChip(title: "横条", selected: !covers) { covers = false }
                LibraryChip(title: "大封面", selected: covers) { covers = true }
            }.padding(.horizontal, 28)
            if covers {
                LazyVGrid(columns: LibraryAppearance.columns, alignment: .leading, spacing: 22) {
                    ForEach(songs) { track in LibrarySongCoverTile(track: track, queue: songs) }
                }.padding(.horizontal, 28)
            } else {
                TrackList(tracks: songs, showArtist: true).padding(.horizontal, 28)
            }
        }
        if !records.isEmpty {
            SectionHeader(title: "专辑和 EP", detail: "最新在前").padding(.horizontal, 28)
            ScrollView(.horizontal) {
                LazyHStack(alignment: .top, spacing: 18) {
                    ForEach(records) { group in
                        CoverTile(coverPath: group.coverPath, title: group.kind == "ep" ? PersonReleases.releaseTitle(group.title) : group.title,
                                  subtitle: [String(group.year.prefix(4)), group.kind == "ep" ? "EP" : "\(group.tracks.count) 首"]
                                    .filter { !$0.isEmpty }.joined(separator: " · "), size: 140) {
                            if let id = group.albumId, let album = albums.first(where: { $0.id == id }) { app.show(.album(album)) }
                            else { tab = .works }
                        }
                    }
                }.padding(.horizontal, 28).padding(.vertical, 6)
            }.frame(height: 200).scrollIndicators(.never)
        }
        if !members.isEmpty { memberShelf }
        if !collaborators.isEmpty { collaboratorShelf }
    }

    @ViewBuilder
    private func worksContent(sections: [WorkSection], queue: [Track]) -> some View {
        ScrollView(.horizontal) {
            HStack(spacing: 6) {
                LibraryChip(title: "全部", selected: role.isEmpty) { role = "" }
                ForEach(roles, id: \.self) { value in
                    LibraryChip(title: roleLabels[value] ?? value, selected: role == value,
                                dot: LibraryAppearance.roleColor(value)) { role = value }
                }
            }.padding(.horizontal, 28)
        }.frame(height: 30).scrollIndicators(.never)
        if sections.isEmpty { ContentUnavailableView("没有作品记录", systemImage: "music.note") }
        ForEach(sections) { section in
            LibraryPersonWorkSection(section: section, queue: queue).padding(.horizontal, 28)
        }
    }

    @ViewBuilder private var peopleContent: some View {
        if let person {
            PersonAdminView(personId: personId, name: person.name, person: person).padding(.horizontal, 28)
        }
        if !collaborators.isEmpty { collaboratorShelf }
        else { Text("没有合作者。关系的多少取决于数据本身。").foregroundStyle(.secondary).padding(.horizontal, 28) }
    }

    private var memberShelf: some View {
        VStack(alignment: .leading, spacing: 12) {
            SectionHeader(title: "成员", detail: "\(members.count) 人").padding(.horizontal, 28)
            ScrollView(.horizontal) {
                LazyHStack(alignment: .top, spacing: 18) {
                    ForEach(members, id: \.memberId) { member in
                        CoverTile(coverPath: member.memberImage, title: member.memberName,
                                  subtitle: [member.part, LibraryIndex.spanOf(member.joined, member.leftAt)].filter { !$0.isEmpty }.joined(separator: " · "),
                                  size: 96, circle: true, placeholder: "person.fill") { app.show(.person(member.memberId)) }
                    }
                }.padding(.horizontal, 28).padding(.vertical, 6)
            }.frame(height: 156).scrollIndicators(.never)
        }
    }

    private var collaboratorShelf: some View {
        VStack(alignment: .leading, spacing: 12) {
            SectionHeader(title: tab == .overview ? "常一起署名" : "合作者", detail: "\(collaborators.count) 人").padding(.horizontal, 28)
            ScrollView(.horizontal) {
                LazyHStack(alignment: .top, spacing: 18) {
                    ForEach(collaborators) { collaborator in
                        CoverTile(coverPath: collaborator.imagePath, title: collaborator.name,
                                  subtitle: "\(collaborator.sharedTracks) 首 · \(collaborator.roles.map { roleLabels[$0] ?? $0 }.joined(separator: "/"))",
                                  size: 96, circle: true, placeholder: "person.fill") { app.show(.person(collaborator.personId)) }
                    }
                }.padding(.horizontal, 28).padding(.vertical, 6)
            }.frame(height: 156).scrollIndicators(.never)
        }
    }

    private func load() async {
        guard let client = app.core else { loading = false; return }
        loading = true
        defer { loading = false }
        do {
            async let directory = client.peopleDirectory()
            async let loadedWorks = client.personWorks(personId)
            async let peers = client.collaborators(personId)
            async let records = client.albums()
            let entries = try await directory
            person = entries.first { $0.id == personId }
            works = try await loadedWorks
            collaborators = try await peers
            albums = try await records
            memberships = try await app.library.client?.memberships() ?? []
            if person != nil { fallback = nil }
            else if let band = bands.first { fallback = (band.memberName, band.memberImage) }
            else if let found = try await client.personById(personId) { fallback = (found.name, found.imagePath) }
            else { fallback = nil }
        } catch { app.errorMessage = error.localizedDescription }
    }

    private func openVisualization(_ mode: LibraryMode) { app.libraryMode = mode; app.libraryPath = [] }
}

@MainActor
private struct LibraryPersonWorkSection: View {
    let section: WorkSection
    let queue: [Track]
    @State private var expanded: Bool

    init(section: WorkSection, queue: [Track]) {
        self.section = section; self.queue = queue
        _expanded = State(initialValue: !section.collapsed)
    }

    var body: some View {
        LazyVStack(alignment: .leading, spacing: 18) {
            if !section.title.isEmpty {
                Button { expanded.toggle() } label: {
                    Label("\(section.title) · \(section.works.count) 首", systemImage: expanded ? "chevron.down" : "chevron.right")
                }.buttonStyle(.plain).font(.headline)
            }
            if expanded || section.title.isEmpty {
                if !section.note.isEmpty { Text(section.note).font(.caption).foregroundStyle(.secondary) }
                ForEach(PersonReleases.groupByAlbum(section.works.map(\.track))) { group in
                    HStack(alignment: .bottom, spacing: 18) {
                        CoverImage(path: group.coverPath, size: 96, cornerRadius: 8)
                            .shadow(color: .black.opacity(0.2), radius: 10, y: 4)
                        VStack(alignment: .leading, spacing: 6) {
                            Text(group.title).font(.headline)
                            Text([String(group.year.prefix(4)), "\(group.tracks.count) 首"].filter { !$0.isEmpty }.joined(separator: " · "))
                                .font(.caption).foregroundStyle(.secondary)
                        }
                    }.padding(.top, 8)
                    TrackList(tracks: group.tracks, showArtist: true, queue: queue)
                }
            }
        }
    }
}

@MainActor
private struct LibrarySongCoverTile: View {
    @Environment(AppModel.self) private var app
    let track: Track
    let queue: [Track]
    @State private var hovering = false
    private var current: Bool { app.player.track?.id == track.id }

    var body: some View {
        Button { Task { await app.play(songId: track.id, queue: queue) } } label: {
            VStack(alignment: .leading, spacing: 6) {
                CoverImage(path: track.coverPath, size: 140, cornerRadius: 8)
                    .overlay {
                        ZStack {
                            RoundedRectangle(cornerRadius: 8).fill(.black.opacity(0.4))
                            Image(systemName: "play.fill").font(.largeTitle).foregroundStyle(.white)
                        }.opacity(hovering || current ? 1 : 0)
                    }
                    .shadow(color: .black.opacity(0.2), radius: 8, y: 4)
                Text(track.title).font(.callout.weight(.medium)).foregroundStyle(current ? Color.accentColor : Color.primary).lineLimit(1)
                Text([String(track.year.prefix(4)), track.durationSec.map(formatTime) ?? ""].filter { !$0.isEmpty }.joined(separator: " · "))
                    .font(.caption).foregroundStyle(.secondary).lineLimit(1)
            }.frame(width: 140).contentShape(Rectangle())
        }
        .buttonStyle(.plain).onHover { hovering = $0 }.help("播放「\(track.title)」")
        .contextMenu {
            Button("歌曲详情") { app.show(.song(track.id)) }
            FavoriteMenuItem(songId: track.id)
        }
    }
}

@MainActor
private struct LibraryArtistHero<Actions: View>: View {
    @Environment(\.colorScheme) private var colorScheme
    let name: String
    let imagePath: String
    let person: DirectoryEntry?
    let isBand: Bool
    @ViewBuilder var actions: () -> Actions

    var body: some View {
        GeometryReader { proxy in
            ZStack(alignment: .bottomLeading) {
                LibraryArtworkBackdrop(path: imagePath)
                portrait(width: proxy.size.width, height: proxy.size.height)
                LinearGradient(colors: [scrim.opacity(0.8), .clear], startPoint: .bottom, endPoint: .top)
                LinearGradient(colors: [scrim.opacity(0.65), .clear], startPoint: .leading, endPoint: .trailing)
                information(width: proxy.size.width)
                    .padding(.horizontal, 28).padding(.bottom, 26).padding(.top, 24)
            }.frame(width: proxy.size.width, height: proxy.size.height).background(Color.accentColor.opacity(0.04)).clipped()
        }.frame(height: 340)
    }

    private func portrait(width: CGFloat, height: CGFloat) -> some View {
        HStack {
            Spacer(minLength: 0)
            if imagePath.isEmpty {
                CoverImage(path: "", size: 150, circle: true, placeholder: "person.fill", identity: name)
                    .padding(.trailing, 56).opacity(0.65)
            } else {
                CoverImage(path: imagePath, size: height * 1.4, cornerRadius: 0)
                    .frame(width: min(width * 0.7, height * 1.4), height: height).clipped()
                    .mask(LinearGradient(stops: [.init(color: .clear, location: 0), .init(color: .black, location: 0.45)],
                                         startPoint: .leading, endPoint: .trailing))
                    .opacity(width < 760 ? 0.4 : 0.85)
            }
        }.frame(height: height).allowsHitTesting(false).accessibilityHidden(true)
    }

    private var scrim: Color { colorScheme == .dark ? .black : .white }

    private func information(width: CGFloat) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            roleTags
            Text(name.isEmpty ? "加载中…" : name)
                .font(.system(size: titleSize(width: width), weight: .bold))
                .lineLimit(2).minimumScaleFactor(0.7).textSelection(.enabled)
            if let person {
                HStack(spacing: 26) {
                    if person.performer > 0 { StatBlock(value: "\(person.performer)", label: "演唱") }
                    StatBlock(value: "\(person.songs)", label: "首作品")
                    if !person.firstYear.isEmpty { StatBlock(value: String(person.firstYear.prefix(4)), label: "最早发行") }
                }
            }
            actions()
        }.frame(maxWidth: width < 760 ? .infinity : min(680, width * 0.72), alignment: .leading)
    }

    private var roleTags: some View {
        ScrollView(.horizontal) {
            HStack(spacing: 6) {
                if isBand { tag("乐队", color: .secondary) }
                if let person {
                    ForEach(LibraryIndex.rolesOf(person), id: \.self) { role in
                        tag(roleLabels[role] ?? role, color: LibraryAppearance.roleColor(role))
                    }
                    if !person.firstYear.isEmpty {
                        Text(years(person)).font(.caption).foregroundStyle(.secondary)
                    }
                }
            }
        }.frame(height: 28).scrollIndicators(.never)
    }

    private func tag(_ title: String, color: Color) -> some View {
        Text(title).font(.caption).padding(.horizontal, 10).padding(.vertical, 5)
            .foregroundStyle(color).background(color.opacity(0.1), in: RoundedRectangle(cornerRadius: 6))
            .overlay { RoundedRectangle(cornerRadius: 6).strokeBorder(color.opacity(0.4), lineWidth: 0.5) }
    }

    private func titleSize(width: CGFloat) -> CGFloat {
        if name.count > 9 { return min(56, max(30, width * 0.046)) }
        return min(72, max(34, width * 0.065))
    }

    private func years(_ person: DirectoryEntry) -> String {
        let first = String(person.firstYear.prefix(4))
        let last = String((person.ended.isEmpty ? person.lastYear : person.ended).prefix(4))
        let span = first == last || last.isEmpty ? first : "\(first)–\(last)"
        return person.ended.isEmpty ? span : "\(span) · 已结束活动"
    }
}
