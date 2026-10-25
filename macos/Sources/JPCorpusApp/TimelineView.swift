import JPCore
import JPCorpusModel
import SwiftUI

@MainActor
struct ReleaseTimelineView: View {
    @Environment(AppModel.self) private var app
    @AppStorage("timeline.perYear") private var perYear = 90.0
    private var model: VisualizationModel { app.visualizations }
    var body: some View {
        @Bindable var model = model
        VStack(spacing: 0) {
            if let id = model.treePerson { TimelineTree(personId: id).id(id) }
            else {
                HStack {
                    TextField("筛选音乐人",text: $model.timelineQuery).textFieldStyle(.roundedBorder).frame(maxWidth: 240)
                    Text("缩放").foregroundStyle(.secondary)
                    Slider(value: $perYear,in: 30...240).frame(width: 160)
                    Text("专辑 · 单曲 · 解散 ┃ · 之后单飞 ╌").font(.caption).foregroundStyle(.secondary)
                    Spacer()
                }.padding(12)
                let layout = model.timeline
                if layout.rows.isEmpty { ContentUnavailableView("还没有带年份的发行",systemImage: "calendar",description: Text("刮削补上发行日期之后这里就有了")) }
                else { overview(layout) }
            }
        }
        .task(id: "\(app.libraryURL?.path ?? "")|\(app.library.version)") {
            if let core = app.core { await model.load(core,key: "\(app.libraryURL?.path ?? "demo")|\(app.library.version)") }
        }
        .onChange(of: model.error) { _, error in if let error { app.errorMessage = error; model.error = nil } }
    }
    private func coverSize(_ r: TimelineRelease) -> Double { r.albumId == nil ? 24 : min(44,26+Double(r.tracks)*1.6) }
    private func overview(_ layout: TimelineLayout) -> some View {
        let rows = layout.rows, connectors = layout.connectors
        let axis = ReleaseTimeline.warpYears(rows.map { $0.releases.map { WarpItem(at: $0.at,size: coverSize($0.release)) } },min: layout.min,max: layout.max,perYear: perYear)
        let width = axis.width+80, height = 30+Double(rows.count)*64+16
        let step = perYear < 45 ? 5 : perYear < 70 ? 2 : 1
        let years = Array(Int(layout.min)...Int(layout.max)).filter { $0 % step == 0 }
        return ScrollViewReader { proxy in
            ScrollView([.horizontal,.vertical]) {
                HStack(alignment: .top,spacing: 0) {
                    VStack(spacing: 0) {
                        Color.clear.frame(height: 30)
                        ForEach(rows) { row in
                            Button { model.treePerson = row.id } label: {
                                HStack(spacing: 8) {
                                    CoverImage(path: row.imagePath,size: row.under == nil ? 32 : 26,circle: true,identity: row.name)
                                    VStack(alignment: .leading) { Text(row.name).lineLimit(1); Text("\(row.isBand ? "乐队" : "个人") · \(row.releases.count) 次发行").font(.caption2).foregroundStyle(.secondary) }
                                    Spacer()
                                }.padding(.leading,row.under == nil ? 8 : 24).frame(width: 200,height: 64)
                            }.buttonStyle(.plain).help("放大看 \(row.name) 的时间树")
                        }
                    }
                    ZStack(alignment: .topLeading) {
                        Canvas { context, _ in
                            for year in years {
                                let x = 40+axis.x(Double(year)); var p = Path(); p.move(to: CGPoint(x: x,y: 26)); p.addLine(to: CGPoint(x: x,y: height))
                                context.stroke(p,with: .color(.secondary.opacity(0.15)),lineWidth: 0.5)
                                context.draw(Text(String(year)).font(.caption).foregroundColor(.secondary),at: CGPoint(x: x,y: 14))
                            }
                            for (i,row) in rows.enumerated() {
                                let y = 30+Double(i)*64+32; var p = Path(); p.move(to: CGPoint(x: 40+axis.x(row.start),y: y)); p.addLine(to: CGPoint(x: 40+axis.x(row.end),y: y))
                                context.stroke(p,with: .color(.accentColor.opacity(0.4)),lineWidth: 2)
                                if let end = row.endedAt { let x = 40+axis.x(end); var mark = Path(); mark.move(to: CGPoint(x: x,y: y-17)); mark.addLine(to: CGPoint(x: x,y: y+17)); context.stroke(mark,with: .color(.orange),lineWidth: 2) }
                            }
                            for c in connectors {
                                let a = CGPoint(x: 40+axis.x(c.fromAt),y: 62+Double(c.fromRow)*64), b = CGPoint(x: 40+axis.x(c.toAt),y: 62+Double(c.toRow)*64)
                                var p = Path(); p.move(to: a); p.addCurve(to: b,control1: CGPoint(x: a.x+20,y: a.y),control2: CGPoint(x: b.x-20,y: b.y))
                                context.stroke(p,with: .color(.orange.opacity(0.7)),style: StrokeStyle(lineWidth: 1.5,dash: [4,4]))
                            }
                        }
                        ForEach(Array(rows.enumerated()),id: \.element.id) { i,row in
                            let centers = ReleaseTimeline.packRow(row.releases.map { PackItem(x: 40+axis.x($0.at),size: coverSize($0.release)) })
                            ForEach(Array(row.releases.enumerated()),id: \.element.release.id) { j,item in
                                let r = item.release, size = coverSize(r)
                                Button { model.treePerson = row.id; model.treeFocus = r.id } label: {
                                    CoverImage(path: r.coverPath,size: size,cornerRadius: 4,circle: r.albumId == nil)
                                }.buttonStyle(.plain).position(x: centers[j],y: 62+Double(i)*64)
                                    .help("\(r.title) · \(r.date) · \(r.tracks) 首；点开时间树")
                            }
                        }
                        Color.clear.frame(width: 1,height: 1).position(x: width,y: 0).id("latest")
                    }.frame(width: width,height: height)
                }
            }.onAppear { proxy.scrollTo("latest",anchor: .trailing) }
        }
    }
}

@MainActor
struct TimelineTree: View {
    @Environment(AppModel.self) private var app
    let personId: Int64
    @AppStorage("timeline.tree.horizontal") private var horizontal = false
    @State private var parts: [TreePart] = []
    @State private var expanded: Set<String> = []
    private var model: VisualizationModel { app.visualizations }
    var body: some View {
        VStack(alignment: .leading,spacing: 12) {
            HStack {
                Button { model.treePerson = nil; model.treeFocus = nil } label: { Label("全部音乐人",systemImage: "chevron.left") }.buttonStyle(.borderless)
                Text(model.directory.byId[personId]?.name ?? "").font(.title2.bold())
                Spacer()
                Picker("方向",selection: $horizontal) { Text("竖向").tag(false); Text("横向").tag(true) }.pickerStyle(.segmented).frame(width: 130)
                Button("打开档案") { app.show(.person(personId)) }
            }.padding(.horizontal,16)
            ScrollViewReader { proxy in
                ScrollView(horizontal ? [.horizontal,.vertical] : [.vertical]) {
                    if horizontal {
                        HStack(alignment: .top,spacing: 28) { treeParts }.padding(24)
                    } else { LazyVStack(alignment: .leading,spacing: 22) { treeParts }.padding(24) }
                }.onChange(of: parts.count) { _, _ in if let id = model.treeFocus { proxy.scrollTo(id,anchor: .center) } }
            }
        }.padding(.top,12)
        .task(id: "\(personId)|\(app.library.version)|\(model.dataVersion)") {
            guard let client = app.core else { return }
            do {
                var works: [Int64:[PersonWork]] = [:]
                let ids = [personId]+(model.directory.membersOf[personId] ?? []).filter { !$0.leftAt.isEmpty }.map(\.memberId)
                for id in ids { works[id] = try await client.personWorks(id) }
                guard !Task.isCancelled else { return }
                parts = ReleaseTimeline.buildTree(personId,model.directory,model.releases,works)
            } catch { if !Task.isCancelled { app.errorMessage = error.localizedDescription } }
        }
    }
    @ViewBuilder private var treeParts: some View {
        ForEach(Array(parts.enumerated()),id: \.element.personId) { _,part in
            VStack(alignment: .leading,spacing: 16) {
                if let after = part.after {
                    VStack(alignment: .leading) { Text("\(LibraryIndex.yearOf(after.ended)) 年 \(after.bandName) 解散").font(.headline); Text("\(part.name) 的个人作品从这里接着长").foregroundStyle(.secondary) }.padding(16).glassCard()
                }
                let cols = ReleaseTimeline.buildAxisColumns(part.items,endYear: model.directory.byId[part.personId].flatMap { ReleaseTimeline.yearValue($0.ended).map { Int(floor($0)) } },year: { Int(floor($0.release.at)) })
                if horizontal { HStack(alignment: .top,spacing: 24) { yearColumns(cols,part: part) } }
                else { LazyVStack(spacing: 18) { yearColumns(cols,part: part) } }
            }
        }
    }
    @ViewBuilder private func yearColumns(_ columns: [AxisColumn<TreeRelease>],part: TreePart) -> some View {
        ForEach(Array(columns.enumerated()),id: \.offset) { _,col in
            switch col {
            case let .gap(from,to): Text("\(from) … \(to)").font(.caption).foregroundStyle(.secondary)
            case let .year(year,items,ended):
                VStack(spacing: 14) {
                    Text("\(year)\(ended ? " · 解散" : "")").font(.headline).foregroundStyle(ended ? .orange : .secondary)
                    ForEach(items,id: \.release.release.id) { item in
                        if horizontal { releaseCard(item).frame(width: 300) }
                        else {
                            let right = (part.items.firstIndex { $0.release.release.id == item.release.release.id } ?? 0) % 2 == 1
                            HStack(spacing: 18) {
                                if right { Spacer(minLength: 0) }
                                releaseCard(item).frame(maxWidth: 420)
                                if !right { Spacer(minLength: 0) }
                            }.padding(.horizontal,24)
                        }
                    }
                }
                .background(alignment: horizontal ? .top : .center) {
                    Rectangle().fill(.tint.opacity(0.16)).frame(width: horizontal ? nil : 2,height: horizontal ? 2 : nil).allowsHitTesting(false)
                }
            }
        }
    }
    private func releaseCard(_ item: TreeRelease) -> some View {
        let r = item.release.release, open = expanded.contains(r.id), songs = open ? item.songs : Array(item.songs.prefix(6))
        return VStack(alignment: .leading,spacing: 10) {
            HStack(alignment: .top) {
                Button { openRelease(r) } label: { CoverImage(path: r.coverPath,size: 64,cornerRadius: 7) }.buttonStyle(.plain)
                VStack(alignment: .leading,spacing: 4) { Button(r.title) { openRelease(r) }.buttonStyle(.plain).font(.headline); Text("\(r.date) · \(r.tracks) 首").font(.caption).foregroundStyle(.secondary) }
            }
            ForEach(songs) { song in
                Button { Task { await app.play(songId: song.id) } } label: {
                    HStack { Image(systemName: app.player.track?.id == song.id && app.player.isPlaying ? "speaker.wave.2.fill" : "play.fill"); Text(song.track.title); Spacer() }
                }.buttonStyle(.borderless)
            }
            if item.songs.count > 6 { Button(open ? "收起" : "还有 \(item.songs.count-6) 首") { if open { expanded.remove(r.id) } else { expanded.insert(r.id) } }.buttonStyle(.borderless) }
        }.padding(16).frame(maxWidth: 560,alignment: .leading).glassCard().id(r.id)
    }
    private func openRelease(_ r: TimelineRelease) {
        if let id = r.albumId { Task { do { if let album = try await app.core?.albums().first(where: { $0.id == id }) { app.show(.album(album)) } } catch { app.errorMessage = error.localizedDescription } } }
        else { Task { await app.play(songId: r.songId) } }
    }
}
