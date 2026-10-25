import JPCore
import JPCorpusModel
import SwiftUI

@MainActor
struct PersonReleaseAxis: View {
    @Environment(AppModel.self) private var app
    let sections: [WorkSection]
    let ended: String
    private var items: [AxisRelease] {
        sections.flatMap { s in PersonReleases.groupByAlbum(s.works.map(\.track)).compactMap { g in
            guard g.year.range(of: "^[0-9]{4}",options: .regularExpression) != nil,let y = Int(g.year.prefix(4)) else { return nil }
            return AxisRelease(group: g,kind: s.key == "own" ? "own" : s.key == "credits" ? "credit" : "band",year: y)
        } }
    }
    var body: some View {
        let columns = ReleaseTimeline.buildAxisColumns(items,endYear: Int(ended.prefix(4)),year: { $0.year })
        ScrollView(.horizontal) {
            HStack(alignment: .top,spacing: 14) {
                ForEach(Array(columns.enumerated()),id: \.offset) { _,col in
                    switch col {
                    case let .gap(from,to): Text("…").foregroundStyle(.secondary).help(Text(verbatim: "\(from)–\(to) 没有发行"))
                    case let .year(y,items,ended):
                        VStack(spacing: 7) {
                            Text(verbatim: "\(y)\(ended ? " 解散" : "")").font(.caption).foregroundStyle(ended ? .orange : .secondary)
                            ForEach(items,id: \.group.id) { item in
                                Button { open(item.group) } label: {
                                    CoverImage(path: item.group.coverPath,size: item.kind == "credit" ? 26 : 34,cornerRadius: 4)
                                        .overlay(RoundedRectangle(cornerRadius: 4).stroke(item.kind == "band" ? Color.gray : item.kind == "credit" ? Color.green : Color.clear,lineWidth: 2))
                                }.buttonStyle(.plain).help("\(item.group.title) · \(item.group.year)")
                            }
                        }
                    }
                }
            }.padding(.vertical,8)
        }
    }
    private func open(_ g: AlbumGroup) {
        if let id = g.albumId { Task { if let album = try? await app.core?.albums().first(where: { $0.id == id }) { app.show(.album(album)) } } }
        else if let t = g.tracks.first { app.show(.song(t.id)) }
    }
}
private struct AxisRelease { var group: AlbumGroup; var kind: String; var year: Int }
