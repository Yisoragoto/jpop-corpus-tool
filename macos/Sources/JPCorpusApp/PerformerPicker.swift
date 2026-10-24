import SwiftUI
import JPCore
import JPCorpusModel

/// Windows PerformerPicker：完整名单、筛选、多选，按名单顺序输出。
@MainActor
struct PerformerPicker: View {
    @Environment(AppModel.self) private var app
    @Binding var selected: [Int64]
    @State private var people: [PersonSummary] = []
    @State private var open = false
    @State private var query = ""
    private var names: [String] { people.filter { selected.contains($0.id) }.map(\.name) }
    private var label: String { selected.isEmpty ? "全部歌手" : (names.count == 1 ? names[0] : "\(names.first ?? "…") 等 \(selected.count) 人") }
    private var visible: [PersonSummary] {
        let q = query.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        return q.isEmpty ? people : people.filter { $0.name.lowercased().contains(q) }
    }
    var body: some View {
        Button(label) { open.toggle() }.help(names.joined(separator: "、"))
            .popover(isPresented: $open) {
                VStack(alignment: .leading, spacing: 10) {
                    HStack { TextField("筛选歌手", text: $query); Button("清除") { selected = [] }.disabled(selected.isEmpty) }
                    ScrollView {
                        LazyVStack(alignment: .leading, spacing: 8) {
                            ForEach(visible) { person in
                                Toggle(isOn: Binding(get: { selected.contains(person.id) }, set: { value in
                                    var ids = Set(selected)
                                    if value { ids.insert(person.id) } else { ids.remove(person.id) }
                                    selected = people.filter { ids.contains($0.id) }.map(\.id)
                                })) {
                                    HStack { CoverImage(path: person.imagePath, size: 26, circle: true, placeholder: "person.fill", identity: person.name); Text(person.name); Spacer(); Text("\(person.trackCount) 首").foregroundStyle(.secondary) }
                                }.toggleStyle(.checkbox)
                            }
                            if visible.isEmpty { Text("没有匹配的歌手").foregroundStyle(.secondary) }
                        }
                    }
                }.padding(16).frame(width: 320, height: 360).glass(.popover)
            }
            .task(id: "\(app.tracks.count)-\(app.library.version)") {
                do { people = try await app.client?.performers() ?? [] }
                catch { app.errorMessage = error.localizedDescription }
            }
    }
}
