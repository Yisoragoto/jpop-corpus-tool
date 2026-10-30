import JPCore
import JPCorpusModel
import SwiftUI

@MainActor
struct LibraryArtistBrowser: View {
    @Environment(AppModel.self) private var app
    let filter: String
    @State private var people: [DirectoryEntry] = []
    @State private var memberships: [Membership] = []
    @State private var behind = false
    @State private var role: BehindRole = .all
    @State private var loading = true
    @AppStorage("jp.library.artistWall") private var wall = true

    var body: some View {
        let directory = LibraryIndex.buildDirectory(people, memberships)
        let musicians = LibraryIndex.filterMusicians(directory.musicians, filter)
        let writers = LibraryIndex.filterBehind(directory.behind, role, filter)
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 2) {
                // 不放「音乐人 · 19 人」的大标题：曲库顶上的切换栏已经说明在看音乐人，大标题只占地方
                controls.padding(.horizontal, 28).padding(.top, 16).padding(.bottom, 18)
                if loading {
                    ProgressView("加载中…").frame(maxWidth: .infinity, minHeight: 120)
                } else if behind {
                    if writers.isEmpty { ContentUnavailableView("没有匹配的人", systemImage: "person") }
                    else {
                        ForEach(writers) { person in
                            LibraryDirectoryPerson(entry: person, subtitle: person.roleSummary)
                                .padding(.horizontal, 28)
                        }
                    }
                } else if musicians.isEmpty {
                    ContentUnavailableView(filter.isEmpty ? "还没有演唱署名" : "没有匹配的音乐人", systemImage: "person.2")
                } else if wall {
                    LazyVGrid(columns: LibraryAppearance.columns, alignment: .leading, spacing: 22) {
                        ForEach(musicians, id: \.row.entry.id) { hit in
                            LibraryMusicianItem(hit: hit, wall: true)
                        }
                    }.padding(.horizontal, 28)
                } else {
                    ForEach(musicians, id: \.row.entry.id) { hit in
                        LibraryMusicianItem(hit: hit, wall: false).padding(.horizontal, 28)
                    }
                }
            }.padding(.bottom, 28)
        }
        .clearScrollBackground()
        .task(id: "\(app.library.version)-\(app.tracks.count)") {
            loading = true
            defer { loading = false }
            do {
                people = try await app.core?.peopleDirectory() ?? []
                memberships = try await app.library.client?.memberships() ?? []
            } catch { app.errorMessage = error.localizedDescription }
        }
    }

    private var controls: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 6) {
                LibraryChip(title: "音乐人", selected: !behind) { behind = false }
                LibraryChip(title: "幕后", selected: behind) { behind = true }
                Spacer()
                if !behind {
                    LibraryChip(title: "横条", selected: !wall) { wall = false }
                    LibraryChip(title: "大封面", selected: wall) { wall = true }
                }
            }
            if behind {
                ScrollView(.horizontal) {
                    HStack(spacing: 6) {
                        ForEach(BehindRole.allCases, id: \.self) { value in
                            LibraryChip(title: value == .all ? "全部" : (roleLabels[value.rawValue] ?? value.rawValue),
                                        selected: role == value,
                                        dot: value == .all ? nil : LibraryAppearance.roleColor(value.rawValue)) { role = value }
                        }
                    }
                }.frame(height: 30).scrollIndicators(.never)
            }
        }
    }
}

@MainActor
private struct LibraryMusicianItem: View {
    @Environment(AppModel.self) private var app
    let hit: MusicianHit
    let wall: Bool
    @State private var open = false
    private var expanded: Bool { open || hit.memberHit }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            if wall {
                CoverTile(coverPath: hit.row.entry.imagePath, title: hit.row.entry.name,
                          subtitle: LibraryIndex.musicianSubtitle(hit.row), size: 140, circle: true, placeholder: "person.fill") {
                    app.show(.person(hit.row.entry.id))
                }
                memberToggle.frame(maxWidth: .infinity)
            } else {
                HStack(spacing: 12) {
                    LibraryDirectoryPerson(entry: hit.row.entry, subtitle: LibraryIndex.musicianSubtitle(hit.row))
                    memberToggle
                }
            }
            if expanded {
                LazyVStack(alignment: .leading, spacing: 4) {
                    ForEach(hit.row.members, id: \.membership.memberId) { member in
                        Button { app.show(.person(member.membership.memberId)) } label: {
                            HStack(spacing: 9) {
                                CoverImage(path: member.entry?.imagePath.isEmpty == false ? (member.entry?.imagePath ?? "") : member.membership.memberImage,
                                           size: 28, circle: true, placeholder: "person.fill", identity: member.membership.memberName)
                                VStack(alignment: .leading, spacing: 2) {
                                    Text(member.membership.memberName).font(.callout).lineLimit(1)
                                    Text(LibraryIndex.memberSubtitle(member)).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                                }
                                Spacer(minLength: 0)
                            }.padding(.vertical, 4).contentShape(Rectangle())
                        }
                        .buttonStyle(.plain).foregroundStyle(member.former ? Color.secondary : Color.primary)
                    }
                }.padding(.leading, wall ? 0 : 48)
            }
        }.frame(maxWidth: .infinity, alignment: .topLeading)
    }

    @ViewBuilder private var memberToggle: some View {
        if !hit.row.members.isEmpty {
            Button { open = !expanded } label: {
                Label("\(hit.row.members.count) 人", systemImage: expanded ? "chevron.down" : "chevron.right")
                    .font(.caption).padding(.horizontal, 8).padding(.vertical, 5)
                    .background(.quaternary, in: Capsule())
            }
            .buttonStyle(.plain).help(expanded ? "收起成员" : "展开成员")
        }
    }
}

@MainActor
private struct LibraryDirectoryPerson: View {
    @Environment(AppModel.self) private var app
    let entry: DirectoryEntry
    let subtitle: String
    @State private var hovering = false

    var body: some View {
        Button { app.show(.person(entry.id)) } label: {
            HStack(spacing: 12) {
                CoverImage(path: entry.imagePath, size: 38, circle: true, placeholder: "person.fill", identity: entry.name)
                VStack(alignment: .leading, spacing: 3) {
                    Text(entry.name).font(.system(size: 14, weight: .medium)).lineLimit(1)
                    Text(subtitle).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                }
                Spacer(minLength: 4)
                HStack(spacing: 4) {
                    ForEach(LibraryIndex.rolesOf(entry), id: \.self) { role in
                        Circle().fill(LibraryAppearance.roleColor(role)).frame(width: 7, height: 7)
                    }
                }
            }
            .padding(8).contentShape(Rectangle())
            .background(Color.primary.opacity(hovering ? 0.055 : 0), in: RoundedRectangle(cornerRadius: 6))
        }
        .buttonStyle(.plain).onHover { hovering = $0 }
    }
}
