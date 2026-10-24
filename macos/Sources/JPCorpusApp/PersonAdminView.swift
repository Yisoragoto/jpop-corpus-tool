import SwiftUI
import UniformTypeIdentifiers
import JPCore
import JPCorpusModel

@MainActor
struct PersonAdminView: View {
    @Environment(AppModel.self) private var app
    let personId: Int64
    let name: String
    let isBand: Bool
    @State private var model = MembersModel()
    @State private var choosing = false
    @State private var editing = false
    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Button { choosing = true } label: { Label("更换照片", systemImage: "photo") }
                Button { Task { _ = await app.library.perform { _ = try await app.library.client?.artistImageReset(name); app.library.version += 1 } } } label: { Label("恢复刮削照片", systemImage: "arrow.counterclockwise") }
            }.buttonStyle(.borderless)
            ForEach(model.bands, id: \.bandId) { band in
                Button("\(band.leftAt.isEmpty ? "也在" : "曾属") \(band.bandName) · \(LibraryIndex.spanOf(band.joined, band.leftAt))") { app.show(.person(band.bandId)) }.buttonStyle(.link)
            }
            if isBand || !model.members.isEmpty || !model.suggestions.isEmpty || editing {
                HStack { Text("成员").font(.headline); Spacer(); Button(editing ? "完成" : "编辑") { editing.toggle() } }
                ForEach(model.members, id: \.memberId) { m in
                    if editing {
                        MemberEditRow(bandId: personId, initial: m, model: model)
                    } else {
                        Button { app.show(.person(m.memberId)) } label: {
                            HStack { CoverImage(path: m.memberImage, size: 34, circle: true, placeholder: "person.fill", identity: m.memberName); Text(m.memberName); Text("\(m.part) · \(LibraryIndex.spanOf(m.joined, m.leftAt))").foregroundStyle(.secondary) }
                        }.buttonStyle(.plain)
                    }
                }
                if editing {
                    MemberEditRow(bandId: personId, initial: nil, model: model).id(model.members.count)
                    Text("日期写年份就行（2019，或 2019-05-27）；现在还在的，离开空着。改过的都记成手动，以后刮削不会盖掉。").font(.caption).foregroundStyle(.secondary)
                }
                ForEach(model.suggestions, id: \.memberId) { s in
                    HStack {
                        Text("\(s.memberName) 可能是成员：写的 \(s.written) 首里 \(s.shared) 首给这个乐队").font(.caption)
                        Button("是成员") { Task { if let c = app.library.client { await model.confirm(personId, member: s.memberId, client: c); app.library.version += 1 } } }
                        Button("不是") { Task { if let c = app.library.client { await model.rejectSuggestion(personId, member: s.memberId, client: c); app.library.version += 1 } } }
                    }
                }
            } else { Button("添加成员") { editing = true } }
            if !model.note.isEmpty { Text(model.note).font(.caption) }
            if let error = model.errorMessage { Text(error).foregroundStyle(.orange) }
            if let error = app.library.errorMessage { Text(error).foregroundStyle(.orange) }
        }.disabled(model.busy)
            .task(id: personId) { if let c = app.library.client { await model.load(personId, client: c) } }
            .fileImporter(isPresented: $choosing, allowedContentTypes: ["jpg", "jpeg", "png", "webp"].compactMap { UTType(filenameExtension: $0) }) { result in
                switch result {
                case .failure(let error): app.library.errorMessage = error.localizedDescription
                case .success(let url): Task { _ = await app.library.perform { _ = try await app.library.client?.artistImageSet(name, path: url.path); app.library.version += 1 } }
                }
            }
    }
}

@MainActor
struct MemberEditRow: View {
    @Environment(AppModel.self) private var app
    let bandId: Int64
    let initial: Membership?
    let model: MembersModel
    @State private var name: String
    @State private var part: String
    @State private var joined: String
    @State private var leftAt: String
    init(bandId: Int64, initial: Membership?, model: MembersModel) {
        self.bandId = bandId; self.initial = initial; self.model = model
        _name = State(initialValue: initial?.memberName ?? ""); _part = State(initialValue: initial?.part ?? "")
        _joined = State(initialValue: initial?.joined ?? ""); _leftAt = State(initialValue: initial?.leftAt ?? "")
    }
    private var changed: Bool {
        guard let initial else { return !name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }
        return part != initial.part || joined != initial.joined || leftAt != initial.leftAt
    }
    var body: some View {
        HStack {
            if initial == nil { TextField("新成员名字", text: $name) } else { Text(name).frame(width: 100) }
            TextField("分工", text: $part)
            TextField("加入", text: $joined).frame(width: 90)
            TextField("离开（在队留空）", text: $leftAt).frame(width: 120)
            Button(initial == nil ? "添加" : "保存") {
                Task {
                    guard let c = app.library.client else { return }
                    let input = MembershipInput(bandId: bandId, memberId: initial?.memberId, memberName: name.trimmingCharacters(in: .whitespacesAndNewlines), part: part.trimmingCharacters(in: .whitespacesAndNewlines), joined: joined.trimmingCharacters(in: .whitespacesAndNewlines), leftAt: leftAt.trimmingCharacters(in: .whitespacesAndNewlines))
                    if await model.put(input, client: c) { app.library.version += 1; if initial == nil { name = ""; part = ""; joined = ""; leftAt = "" } }
                }
            }.disabled(!changed || model.busy || !LibraryIndex.validDate(joined.trimmingCharacters(in: .whitespacesAndNewlines)) || !LibraryIndex.validDate(leftAt.trimmingCharacters(in: .whitespacesAndNewlines)))
            if let initial {
                if model.removal == initial.memberId {
                    Text("不是成员？").font(.caption)
                    Button("确认", role: .destructive) { Task { if let c = app.library.client { await model.removeConfirmed(bandId, client: c); app.library.version += 1 } } }
                    Button("取消") { model.removal = nil }
                } else { Button("移除", role: .destructive) { model.removal = initial.memberId } }
            }
        }
    }
}
