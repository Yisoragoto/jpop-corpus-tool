import SwiftUI
import JPCore
import JPCorpusModel

@MainActor
struct ArtistRosterView: View {
    @Environment(AppModel.self) private var app
    @State private var roster: [ArtistRow] = []
    @State private var people: [DirectoryEntry] = []
    @State private var busy: String?
    @State private var note = ""
    var body: some View {
        DisclosureGroup("歌手资料与照片（\(roster.count) 人）") {
            LazyVStack(alignment: .leading, spacing: 12) {
                ForEach(roster, id: \.name) { artist in
                    HStack {
                        CoverImage(path: artist.hasImage ? artist.imagePath : "", size: 40, circle: true, placeholder: "person.fill", identity: artist.name)
                        VStack(alignment: .leading) {
                            Button(artist.name) { if let id = people.first(where: { $0.name == artist.name })?.id { app.show(.person(id)) } }.buttonStyle(.plain)
                            Text(["\(artist.trackCount) 首", artist.artistType, artist.country, artist.formed].filter { !$0.isEmpty }.joined(separator: " · ")).font(.caption).foregroundStyle(.secondary)
                        }
                        Spacer()
                        Button(busy == artist.name ? "正在查…" : "刮削资料与照片") { Task { await scrape(artist.name) } }.disabled(busy != nil)
                    }
                }
                if !note.isEmpty { Text(note).font(.caption) }
            }.padding(.top, 10)
        }
        .task(id: app.library.version) {
            do { roster = try await app.core?.artistRoster() ?? []; people = try await app.core?.peopleDirectory() ?? [] }
            catch { app.errorMessage = error.localizedDescription }
        }
    }
    private func scrape(_ name: String) async {
        guard let client = app.core else { return }
        busy = name; defer { busy = nil }
        do {
            let result = try await client.scrapeArtist(name)
            note = result.notFound ? "\(name)：没有找到" : "\(name)：资料已更新，写入 \(result.memberships) 条成员关系"
            app.library.version += 1
        } catch { app.errorMessage = error.localizedDescription }
    }
}
