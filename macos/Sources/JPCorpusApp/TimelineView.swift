import AppKit
import JPCore
import JPCorpusModel
import SwiftUI

@MainActor
struct ReleaseTimelineView: View {
    @Environment(AppModel.self) private var app
    @AppStorage("timeline.perYear") private var perYear = 90.0
    /// 内容相对视口的偏移（都 ≥ 0）。不用 ScrollView：它没法缩放时钉住光标，也冻结不了左边的名字和上面的年份
    @State private var pan = CGPoint.zero
    @State private var dragStart: CGPoint?
    @State private var pinchStart: Double?
    @State private var startedAtLatest = false
    @State private var appeared = false
    @State private var hoveredRelease: String?
    @State private var viewport = CGSize.zero
    private var model: VisualizationModel { app.visualizations }
    private let nameWidth = 200.0, headerHeight = 30.0, rowHeight = 64.0, inset = 40.0
    private static let zoomRange = 30.0...240.0

    var body: some View {
        @Bindable var model = model
        VStack(spacing: 0) {
            if let id = model.treePerson {
                TimelineTree(personId: id).id(id)
                    .transition(.asymmetric(insertion: .opacity.combined(with: .scale(scale: 0.98, anchor: .top)), removal: .opacity))
            } else {
                VStack(spacing: 0) {
                    ViewThatFits(in: .horizontal) {
                        HStack { timelineFilters; Spacer(); legend }
                        VStack(alignment: .leading, spacing: 10) { timelineFilters; legend }
                    }.padding(12)
                    let layout = model.timeline
                    if layout.rows.isEmpty { ContentUnavailableView("还没有带年份的发行",systemImage: "calendar",description: Text("刮削补上发行日期之后这里就有了")) }
                    else { overview(layout) }
                }
                .transition(.opacity)
            }
        }
        .animation(.smooth(duration: 0.35), value: model.treePerson)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .task(id: "\(app.libraryURL?.path ?? "")|\(app.library.version)") {
            if let core = app.core { await model.load(core,key: "\(app.libraryURL?.path ?? "demo")|\(app.library.version)") }
        }
        .onChange(of: model.error) { _, error in if let error { app.errorMessage = error; model.error = nil } }
    }
    private var timelineFilters: some View {
        @Bindable var model = model
        return HStack {
            TextField("筛选音乐人",text: $model.timelineQuery).textFieldStyle(.roundedBorder).frame(width: 200)
            Text("缩放").foregroundStyle(.secondary)
            Slider(value: Binding(get: { perYear }, set: { zoomTo($0, anchor: nil) }),in: Self.zoomRange).frame(width: 160)
                .help("也可以双指捏合，或按住 ⌘ 滚动，以光标为中心缩放")
        }
    }
    private var legend: some View {
        HStack(spacing: 14) {
            HStack(spacing: 6) {
                RoundedRectangle(cornerRadius: 3).fill(LinearGradient(colors: [.cyan, .purple], startPoint: .topLeading, endPoint: .bottomTrailing)).frame(width: 11, height: 11)
                Text("专辑")
            }
            HStack(spacing: 6) { Circle().fill(.tint).frame(width: 9, height: 9); Text("单曲（越大歌越多）") }
            HStack(spacing: 6) { Rectangle().fill(.orange).frame(width: 2, height: 12); Text("解散") }
            HStack(spacing: 6) {
                Path { p in p.move(to: CGPoint(x: 0, y: 6)); p.addLine(to: CGPoint(x: 18, y: 6)) }
                    .stroke(.secondary, style: StrokeStyle(lineWidth: 1.5, dash: [4, 3])).frame(width: 18, height: 12)
                Text("之后单飞")
            }
        }.font(.caption).foregroundStyle(.secondary).fixedSize()
    }
    private func coverSize(_ r: TimelineRelease) -> Double { r.albumId == nil ? 24 : min(44,26+Double(r.tracks)*1.6) }
    private func axis(_ layout: TimelineLayout, perYear: Double) -> WarpedYears {
        ReleaseTimeline.warpYears(layout.rows.map { $0.releases.map { WarpItem(at: $0.at,size: coverSize($0.release)) } },min: layout.min,max: layout.max,perYear: perYear)
    }
    private func contentSize(_ layout: TimelineLayout, _ axis: WarpedYears) -> CGSize {
        CGSize(width: axis.width+inset*2, height: Double(layout.rows.count)*rowHeight+16)
    }

    // MARK: - 平移与缩放

    private var bodyViewport: CGSize { CGSize(width: max(0,viewport.width-nameWidth), height: max(0,viewport.height-headerHeight)) }

    private func clamped(_ p: CGPoint) -> CGPoint {
        let layout = model.timeline, size = contentSize(layout, axis(layout, perYear: perYear))
        return CGPoint(x: TimelinePan.clamp(p.x, content: size.width, viewport: bodyViewport.width),
                       y: TimelinePan.clamp(p.y, content: size.height, viewport: bodyViewport.height))
    }
    private func panBy(_ dx: Double, _ dy: Double) { pan = clamped(CGPoint(x: pan.x+dx, y: pan.y+dy)) }
    /// anchor：光标在时间区（不含左侧名字列）里的横坐标；nil 表示视口正中
    private func zoomTo(_ next: Double, anchor: Double?) {
        let layout = model.timeline
        let value = min(Self.zoomRange.upperBound, max(Self.zoomRange.lowerBound, next))
        guard value != perYear else { return }
        let old = axis(layout, perYear: perYear), new = axis(layout, perYear: value)
        let a = anchor ?? bodyViewport.width/2
        perYear = value
        pan = clamped(CGPoint(x: TimelinePan.zoomedOffset(pan.x, anchor: a, inset: inset, old: old, new: new), y: pan.y))
    }

    /// 一次排版要用的东西，拆成几个小视图时一起传
    private struct TimelineFrame {
        let rows: [TimelineRow]
        let connectors: [TimelineConnector]
        let axis: WarpedYears
        let size: CGSize
        let years: [Int]
    }

    private func overview(_ layout: TimelineLayout) -> some View {
        let axis = axis(layout, perYear: perYear)
        let step = perYear < 45 ? 5 : perYear < 70 ? 2 : 1
        let frame = TimelineFrame(rows: layout.rows, connectors: layout.connectors, axis: axis, size: contentSize(layout, axis),
                          years: Array(Int(layout.min)...Int(layout.max)).filter { $0 % step == 0 })
        return GeometryReader { geometry in
            frozenPanes(frame, geometry.size)
                .modifier(TimelineInput(pan: $pan, dragStart: $dragStart, pinchStart: $pinchStart, perYear: perYear, nameWidth: nameWidth,
                                        clamp: clamped, panBy: panBy, zoomTo: zoomTo))
                .onAppear {
                    viewport = geometry.size
                    if !startedAtLatest { startedAtLatest = true; pan = clamped(CGPoint(x: Double.greatestFiniteMagnitude, y: 0)) }
                    appeared = true
                }
                .onChange(of: geometry.size) { _, next in viewport = next; pan = clamped(pan) }
                .onChange(of: frame.rows.count) { _, _ in pan = clamped(pan) }
        }
    }

    /// 四块：可平移的时间区、只横向跟随的年份、只竖向跟随的名字、左上角
    private func frozenPanes(_ frame: TimelineFrame, _ viewport: CGSize) -> some View {
        let offset = pan
        return ZStack(alignment: .topLeading) {
            timeArea(frame)
                .frame(width: frame.size.width, height: frame.size.height, alignment: .topLeading)
                .offset(x: nameWidth-offset.x, y: headerHeight-offset.y)
            yearHeader(frame, offsetX: offset.x)
                .frame(width: max(0, viewport.width-nameWidth), height: headerHeight)
                .background(.bar)
                .offset(x: nameWidth)
            nameColumn(frame.rows)
                .offset(y: headerHeight-offset.y)
                .frame(width: nameWidth, height: viewport.height, alignment: .topLeading)
                .clipped()
                .background(.bar)
            Rectangle().fill(.bar).frame(width: nameWidth, height: headerHeight)
        }
        .frame(width: viewport.width, height: viewport.height, alignment: .topLeading)
        .clipped()
        .contentShape(Rectangle())
    }

    private func timeArea(_ frame: TimelineFrame) -> some View {
        ZStack(alignment: .topLeading) {
            timeLines(frame)
                .opacity(appeared ? 1 : 0)
                .animation(.easeOut(duration: 0.5), value: appeared)
            ForEach(Array(frame.rows.enumerated()), id: \.element.id) { i, row in
                releaseRow(row, index: i, axis: frame.axis)
            }
        }
    }

    private func timeLines(_ frame: TimelineFrame) -> some View {
        let rowHeight = self.rowHeight, inset = self.inset
        return Canvas { context, _ in
            let axis = frame.axis
            for year in frame.years {
                let x = inset+axis.x(Double(year))
                var p = Path(); p.move(to: CGPoint(x: x, y: 0)); p.addLine(to: CGPoint(x: x, y: frame.size.height))
                context.stroke(p, with: .color(.secondary.opacity(0.15)), lineWidth: 0.5)
            }
            for (i, row) in frame.rows.enumerated() {
                let y = Double(i)*rowHeight+rowHeight/2
                var p = Path(); p.move(to: CGPoint(x: inset+axis.x(row.start), y: y)); p.addLine(to: CGPoint(x: inset+axis.x(row.end), y: y))
                context.stroke(p, with: .color(.accentColor.opacity(0.4)), style: StrokeStyle(lineWidth: 2, lineCap: .round))
                if let end = row.endedAt {
                    let x = inset+axis.x(end)
                    var mark = Path(); mark.move(to: CGPoint(x: x, y: y-17)); mark.addLine(to: CGPoint(x: x, y: y+17))
                    context.stroke(mark, with: .color(.orange), lineWidth: 2)
                }
            }
            for c in frame.connectors {
                let a = CGPoint(x: inset+axis.x(c.fromAt), y: rowHeight/2+Double(c.fromRow)*rowHeight)
                let b = CGPoint(x: inset+axis.x(c.toAt), y: rowHeight/2+Double(c.toRow)*rowHeight)
                var p = Path(); p.move(to: a); p.addCurve(to: b, control1: CGPoint(x: a.x+20, y: a.y), control2: CGPoint(x: b.x-20, y: b.y))
                context.stroke(p, with: .color(.orange.opacity(0.7)), style: StrokeStyle(lineWidth: 1.5, dash: [4, 4]))
            }
        }
    }

    private func releaseRow(_ row: TimelineRow, index i: Int, axis: WarpedYears) -> some View {
        let centers = ReleaseTimeline.packRow(row.releases.map { PackItem(x: inset+axis.x($0.at), size: coverSize($0.release)) })
        return ForEach(Array(row.releases.enumerated()), id: \.element.release.id) { j, item in
            releaseCover(item.release, row: row, delay: min(0.6, Double(i)*0.025+Double(j)*0.008))
                .position(x: centers[j], y: rowHeight/2+Double(i)*rowHeight)
        }
    }

    private func releaseCover(_ r: TimelineRelease, row: TimelineRow, delay: Double) -> some View {
        let hovered = hoveredRelease == r.id
        let scale: CGFloat = appeared ? (hovered ? 1.18 : 1) : 0.4
        return Button { model.treePerson = row.id; model.treeFocus = r.id } label: {
            CoverImage(path: r.coverPath, size: coverSize(r), cornerRadius: 4, circle: r.albumId == nil)
                .shadow(color: .black.opacity(hovered ? 0.35 : 0), radius: 6, y: 2)
        }
        .buttonStyle(.plain)
        .scaleEffect(scale)
        .opacity(appeared ? 1 : 0)
        .animation(.spring(response: 0.45, dampingFraction: 0.72).delay(delay), value: appeared)
        .animation(.spring(response: 0.25, dampingFraction: 0.7), value: hovered)
        .onHover { inside in
            if inside { hoveredRelease = r.id } else if hoveredRelease == r.id { hoveredRelease = nil }
        }
        .zIndex(hovered ? 1 : 0)
        .help("\(r.title) · \(r.date) · \(r.tracks) 首；点开时间树")
    }

    private func yearHeader(_ frame: TimelineFrame, offsetX: Double) -> some View {
        let inset = self.inset, headerHeight = self.headerHeight
        return Canvas { context, canvas in
            for year in frame.years {
                let x = inset+frame.axis.x(Double(year))-offsetX
                guard x > -40, x < canvas.width+40 else { continue }
                context.draw(Text(String(year)).font(.caption.monospacedDigit()).foregroundColor(.secondary), at: CGPoint(x: x, y: headerHeight/2))
            }
        }
    }

    private func nameColumn(_ rows: [TimelineRow]) -> some View {
        VStack(spacing: 0) {
            ForEach(Array(rows.enumerated()), id: \.element.id) { i, row in
                nameRow(row)
                    .opacity(appeared ? 1 : 0)
                    .offset(x: appeared ? 0 : -16)
                    .animation(.easeOut(duration: 0.35).delay(min(0.5, Double(i)*0.03)), value: appeared)
            }
        }
    }

    private func nameRow(_ row: TimelineRow) -> some View {
        Button { model.treePerson = row.id } label: {
            HStack(spacing: 8) {
                CoverImage(path: row.imagePath, size: row.under == nil ? 32 : 26, circle: true, identity: row.name)
                VStack(alignment: .leading) {
                    Text(row.name).lineLimit(1)
                    Text("\(row.isBand ? "乐队" : "个人") · \(row.releases.count) 次发行").font(.caption2).foregroundStyle(.secondary)
                }
                Spacer(minLength: 0)
            }
            .padding(.leading, row.under == nil ? 8 : 24)
            .frame(width: nameWidth, height: rowHeight)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .help("放大看 \(row.name) 的时间树")
    }
}

/// 时间轴的输入：滚轮/触控板平移，⌘ 滚动或捏合缩放，拖动平移
@MainActor
private struct TimelineInput: ViewModifier {
    @Binding var pan: CGPoint
    @Binding var dragStart: CGPoint?
    @Binding var pinchStart: Double?
    let perYear: Double
    let nameWidth: Double
    let clamp: (CGPoint) -> CGPoint
    let panBy: (Double, Double) -> Void
    let zoomTo: (Double, Double?) -> Void

    func body(content: Content) -> some View {
        content
            .background(TimelineWheel { location, dx, dy, zoom in
                if zoom { zoomTo(perYear*exp(dy*0.01), location.x-nameWidth) } else { panBy(-dx, -dy) }
            })
            .keepsWindowStill()
            .gesture(DragGesture(minimumDistance: 3).onChanged { value in
                if dragStart == nil { dragStart = pan }
                if let start = dragStart { pan = clamp(CGPoint(x: start.x-value.translation.width, y: start.y-value.translation.height)) }
            }.onEnded { _ in dragStart = nil })
            .simultaneousGesture(MagnifyGesture().onChanged { value in
                if pinchStart == nil { pinchStart = perYear }
                if let start = pinchStart { zoomTo(start*value.magnification, value.startLocation.x-nameWidth) }
            }.onEnded { _ in pinchStart = nil })
    }
}

/// 时间区里的滚轮和触控板：平时平移（两个方向都行），按住 ⌘ 时以光标为中心缩放。
/// 只接管这块区域里的事件；离开页面即移除监视器
@MainActor
private struct TimelineWheel: NSViewRepresentable {
    var handle: (CGPoint, Double, Double, Bool) -> Void
    func makeCoordinator() -> Coordinator { Coordinator(handle) }
    func makeNSView(context: Context) -> NSView { let view = NSView(); context.coordinator.view = view; context.coordinator.install(); return view }
    func updateNSView(_ view: NSView, context: Context) { context.coordinator.handle = handle }
    static func dismantleNSView(_ view: NSView, coordinator: Coordinator) { coordinator.remove() }
    @MainActor final class Coordinator {
        weak var view: NSView?
        var monitor: Any?
        var handle: (CGPoint, Double, Double, Bool) -> Void
        init(_ handle: @escaping (CGPoint, Double, Double, Bool) -> Void) { self.handle = handle }
        func install() {
            monitor = NSEvent.addLocalMonitorForEvents(matching: .scrollWheel) { [weak self] event in
                MainActor.assumeIsolated {
                    guard let self, let view = self.view, let window = view.window, event.window === window, window.attachedSheet == nil else { return event }
                    let p = view.convert(event.locationInWindow, from: nil)
                    guard view.bounds.contains(p) else { return event }
                    // 鼠标滚轮一格只有一两个点，触控板给的是精确像素：滚轮放大到一格约 40 点
                    let k = event.hasPreciseScrollingDeltas ? 1.0 : 40.0
                    self.handle(CGPoint(x: p.x, y: view.bounds.height-p.y), Double(event.scrollingDeltaX)*k, Double(event.scrollingDeltaY)*k, event.modifierFlags.contains(.command))
                    return nil
                }
            }
        }
        func remove() { if let monitor { NSEvent.removeMonitor(monitor) }; monitor = nil }
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
            BackBar(title: "全部音乐人") { model.treePerson = nil; model.treeFocus = nil }
            HStack {
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
                    Text(verbatim: "\(year)\(ended ? " · 解散" : "")").font(.headline).foregroundStyle(ended ? .orange : .secondary)
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
