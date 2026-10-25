import JPCore
import JPCorpusModel
import SwiftUI

@MainActor
struct SubcorpusPicker: View {
    @Environment(AppModel.self) private var app
    var changed: (() -> Void)?
    var body: some View {
        Group {
            if !app.scopes.saved.isEmpty || app.scopes.active != nil {
                Picker("子语料",selection: Binding<Int64?>(get: { app.scopes.active?.id },set: { app.scopes.select($0); changed?() })) {
                    Text("整个库").tag(Int64?.none)
                    ForEach(app.scopes.saved) { s in Text("\(s.name)（\(s.songCount) 首）").tag(Optional(s.id)) }
                }.fixedSize().help("只在这块子语料里找；在分析页保存条件")
            }
        }.task(id: app.libraryURL) { do { if let client = app.core { try await app.scopes.reload(client) } } catch { app.errorMessage = error.localizedDescription } }
    }
}

@MainActor
struct SubcorpusFilters: View {
    @Environment(AppModel.self) private var app
    @Binding var filter: StatsFilter
    @State private var open = false
    var body: some View {
        Button { open.toggle() } label: { Label("子语料条件",systemImage: "line.3.horizontal.decrease") }
            .buttonStyle(.borderless)
            .popover(isPresented: $open) { SubcorpusEditor(filter: $filter).frame(width: 540).padding(20) }
    }
}

@MainActor
private struct SubcorpusEditor: View {
    @Environment(AppModel.self) private var app
    @Binding var filter: StatsFilter
    @State private var facets: FilterFacets?
    @State private var preview: ScopePreview?
    @State private var naming: String?
    @State private var busy = false
    private var active: SavedSubcorpus? { app.scopes.saved.first { SubcorpusScope.sameFilter($0.filter,filter) } }
    var body: some View {
        VStack(alignment: .leading,spacing: 14) {
            Text("子语料").font(.title2.bold())
            Picker("存过的条件",selection: Binding<Int64?>(get: { active?.id },set: { id in
                if let saved = app.scopes.saved.first(where: { $0.id == id }) { filter = saved.filter }
                else { let jpOnly = filter.jpOnly; filter = StatsFilter(); filter.jpOnly = jpOnly }
            })) {
                Text(active != nil || SubcorpusScope.isWholeCorpus(filter) ? "整个库" : "（未保存的条件）").tag(Int64?.none)
                ForEach(app.scopes.saved) { s in Text("\(s.name)（\(s.songCount) 首）").tag(Optional(s.id)) }
            }
            PerformerPicker(selected: $filter.performerIds)
            HStack {
                Text("发行年份"); TextField(facets?.yearMin.map(String.init) ?? "起",text: year(\.yearFrom)).frame(width: 75)
                Text("–"); TextField(facets?.yearMax.map(String.init) ?? "止",text: year(\.yearTo)).frame(width: 75)
            }.help("包含两端；设了年份时，年份不明的歌不算进来")
            Picker("流派",selection: Binding(get: { filter.genres.first ?? "" },set: { filter.genres = $0.isEmpty ? [] : [$0] })) {
                Text("全部流派").tag(""); ForEach(facets?.genres ?? []) { Text("\($0.value)（\($0.count)）").tag($0.value) }
            }
            Picker("语言",selection: Binding(get: { filter.languages.first ?? "" },set: { filter.languages = $0.isEmpty ? [] : [$0] })) {
                Text("全部语言").tag(""); ForEach(facets?.languages ?? []) { Text("\(SubcorpusScope.languageLabels[$0.value] ?? $0.value)（\($0.count)）").tag($0.value) }
            }
            Picker("歌词完整度",selection: $filter.minCompleteness) {
                Text("完整度不限").tag(Double?.none)
                Text("≥ 50%").tag(Optional(0.5)); Text("≥ 80%").tag(Optional(0.8)); Text("≥ 95%").tag(Optional(0.95))
            }
            Toggle("仅日文",isOn: $filter.jpOnly)
            Toggle("副歌只算一遍",isOn: $filter.distinctLines)
            Text(SubcorpusScope.describeFilter(filter)).font(.caption).foregroundStyle(.secondary)
            if let preview { Text("\(preview.songCount) 首 / \(preview.lineCount) 行").font(.caption).foregroundStyle(.secondary) }
            if naming != nil {
                HStack {
                    TextField("名字，比如「2010 年代」",text: Binding(get: { naming ?? "" },set: { naming = $0 })).onSubmit { save() }
                    Button("保存") { save() }.disabled(busy || (naming ?? "").trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                    Button("取消") { naming = nil }
                }
            } else {
                HStack {
                    Button("存为子语料…") { naming = active?.name ?? "" }
                    if let active { Button("删除「\(active.name)」",role: .destructive) { remove(active.id) } }
                }
            }
        }
        .textFieldStyle(.roundedBorder).controlSize(.small)
        .task { do { if let client = app.core { facets = try await client.subcorpusFacets(); try await app.scopes.reload(client) } } catch { app.errorMessage = error.localizedDescription } }
        .task(id: filter) {
            let input = filter
            do { let result = try await app.core?.subcorpusPreview(input); if !Task.isCancelled { preview = result } }
            catch { if !Task.isCancelled { app.errorMessage = error.localizedDescription } }
        }
    }
    private func year(_ key: WritableKeyPath<StatsFilter,Int?>) -> Binding<String> { Binding(get: { filter[keyPath: key].map(String.init) ?? "" },set: { filter[keyPath: key] = Int($0.trimmingCharacters(in: .whitespaces)) }) }
    private func save() {
        guard let name = naming, !name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return }; busy = true; let input = filter
        Task { defer { busy = false }; do { if let client = app.core { let saved = try await client.subcorpusSave(name: name,filter: input); try await app.scopes.reload(client); filter = saved.filter; naming = nil } } catch { app.errorMessage = error.localizedDescription } }
    }
    private func remove(_ id: Int64) { Task { do { if let client = app.core { try await client.subcorpusDelete(id); try await app.scopes.reload(client) } } catch { app.errorMessage = error.localizedDescription } } }
}
