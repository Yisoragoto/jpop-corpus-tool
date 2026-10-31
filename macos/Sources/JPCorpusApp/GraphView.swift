import AppKit
import JPCore
import JPCorpusModel
import SwiftUI

@MainActor
struct GraphView: View {
    @Environment(AppModel.self) private var app
    @State private var scale = 1.0
    @State private var offset = CGPoint.zero
    @State private var dragOrigin: CGPoint?
    @State private var dragging: Int?
    @State private var hovered = -1
    @State private var images: [Int64: Image] = [:]
    private var model: VisualizationModel { app.visualizations }
    private var key: String { "\(app.libraryURL?.path ?? "demo")|\(app.library.version)" }
    private var optionsKey: String { "\(key)|\(model.around ?? -1)|\(model.minShared)|\(model.roles.sorted())|\(model.dataVersion)" }

    var body: some View {
        @Bindable var model = model
        GeometryReader { geometry in
            let graph = model.graph, k = scale, translation = offset, photos = images
            let selected = model.selected, hover = hovered, query = model.graphQuery.lowercased()
            ZStack(alignment: .topLeading) {
                Canvas { context, size in
                    func point(_ n: GraphNode) -> CGPoint { CGPoint(x: n.x*k+translation.x,y: n.y*k+translation.y) }
                    let selectedIndex = graph.nodes.firstIndex { $0.id == selected }
                    let active = graph.nodes.indices.contains(hover) ? hover : selectedIndex
                    let neighbors = active.map { Set(graph.neighbors[$0]+[$0]) }
                    for l in graph.links {
                        let a = point(graph.nodes[l.s]), b = point(graph.nodes[l.t])
                        // 微微弯一点：直线全挤在一起时分不清谁连谁
                        let mid = CGPoint(x: (a.x+b.x)/2-(b.y-a.y)*0.08,y: (a.y+b.y)/2+(b.x-a.x)*0.08)
                        var path = Path(); path.move(to: a); path.addQuadCurve(to: b,control: mid)
                        let lit = active == l.s || active == l.t, dimmed = active != nil && !lit
                        let width = 0.7+log2(Double(max(1,l.shared))+1)*0.6
                        context.stroke(path,with: .color(lit ? Color.accentColor.opacity(0.85) : .secondary.opacity(dimmed ? 0.07 : 0.22)),
                                       style: StrokeStyle(lineWidth: lit ? width+0.8 : width,lineCap: .round,dash: l.member ? [4,4] : []))
                    }
                    for (i,n) in graph.nodes.enumerated() {
                        let p = point(n), r = min(n.r*k,21)*(i == hover ? 1.08 : 1)
                        let rect = CGRect(x: p.x-r,y: p.y-r,width: r*2,height: r*2)
                        var c = context
                        let match = query.isEmpty || n.name.lowercased().contains(query)
                        c.opacity = match && (neighbors == nil || neighbors!.contains(i)) ? 1 : 0.2
                        let color = graphRoleColor(n.role), focused = n.id == selected || i == hover
                        if focused {
                            c.fill(Path(ellipseIn: rect.insetBy(dx: -7,dy: -7)),with: .color(color.opacity(0.22)))
                        }
                        var shadowed = c
                        shadowed.addFilter(.shadow(color: .black.opacity(0.28),radius: focused ? 6 : 3,y: 1))
                        shadowed.fill(Path(ellipseIn: rect),with: .color(Color(nsColor: .windowBackgroundColor)))
                        c.fill(Path(ellipseIn: rect),with: .linearGradient(Gradient(colors: [color.opacity(0.45),color.opacity(0.2)]),startPoint: rect.origin,endPoint: CGPoint(x: rect.maxX,y: rect.maxY)))
                        if let image = photos[n.id] {
                            var clipped = c; clipped.clip(to: Path(ellipseIn: rect)); clipped.draw(image,in: rect)
                        } else {
                            c.draw(Text(String(n.name.prefix(1))).font(.system(size: max(9,r*0.7),weight: .semibold,design: .rounded)).foregroundColor(.primary.opacity(0.85)),at: p)
                        }
                        c.stroke(Path(ellipseIn: rect.insetBy(dx: -1.5,dy: -1.5)),with: .color(color.opacity(focused ? 1 : 0.85)),lineWidth: n.isBand || focused ? 2.5 : 1.2)
                    }
                    var boxes: [CGRect] = []
                    let order = graph.nodes.indices.sorted { a,b in
                        let x = graph.nodes[a].id == selected || a == hover, y = graph.nodes[b].id == selected || b == hover
                        if x != y { return x }; return graph.nodes[a].songs == graph.nodes[b].songs ? a < b : graph.nodes[a].songs > graph.nodes[b].songs
                    }
                    for i in order {
                        let n = graph.nodes[i], priority = n.id == selected || i == hover
                        if !priority && n.r*k < 10 && k < 1.6 { continue }
                        var name = n.name
                        func textWidth(_ name: String) -> Double { context.resolve(Text(name).font(.system(size: 12,weight: .medium))).measure(in: CGSize(width: 10000,height: 100)).width }
                        while textWidth(name) > 144 && name.unicodeScalars.count > 2 { name = String(String.UnicodeScalarView(name.unicodeScalars.dropLast(2)))+"…" }
                        let width = textWidth(name)+12, cx = max(width/2+4,min(size.width-width/2-4,n.x*k+translation.x))
                        let top = n.y*k+min(n.r*k,21)+translation.y+7
                        let box = CGRect(x: cx-width/2,y: top,width: width,height: 20)
                        if top < 0 || box.maxY > size.height || boxes.contains(where: { $0.intersects(box) }) { continue }
                        if !priority && graph.nodes.enumerated().contains(where: { j,other in
                            guard j != i else { return false }; let r = min(other.r*k,21)+3, p = point(other)
                            return box.intersects(CGRect(x: p.x-r,y: p.y-r,width: r*2,height: r*2))
                        }) { continue }
                        boxes.append(box); var c = context
                        let match = query.isEmpty || n.name.lowercased().contains(query)
                        c.opacity = priority ? 1 : match && (neighbors == nil || neighbors!.contains(i)) ? 0.9 : 0.18
                        c.fill(Path(roundedRect: box,cornerRadius: 10),with: .color(Color(nsColor: .windowBackgroundColor).opacity(priority ? 0.92 : 0.7)))
                        c.draw(Text(name).font(.system(size: 12,weight: .medium)).foregroundColor(.primary),at: CGPoint(x: cx,y: top+2),anchor: .top)
                    }
                }
                .background(WheelZoom { point, delta in zoom(at: point,delta: delta) })
                .keepsWindowStill()
                .contentShape(Rectangle())
                .gesture(DragGesture(minimumDistance: 3).onChanged { value in
                    if dragOrigin == nil { dragOrigin = offset; let i = hit(value.startLocation); dragging = i >= 0 ? i : nil }
                    if let i = dragging {
                        model.moveNode(i,x: (value.location.x-offset.x)/scale,y: (value.location.y-offset.y)/scale)
                    } else if let origin = dragOrigin { offset = CGPoint(x: origin.x+value.translation.width,y: origin.y+value.translation.height) }
                }.onEnded { _ in
                    let moved = dragging != nil; dragOrigin = nil; dragging = nil
                    if moved { Task { await model.relaxGraph() } }
                })
                .simultaneousGesture(SpatialTapGesture(count: 2).onEnded { value in let i = hit(value.location); if i >= 0 { app.show(.person(model.graph.nodes[i].id)) } })
                .simultaneousGesture(SpatialTapGesture().onEnded { value in let i = hit(value.location); model.selected = i >= 0 ? model.graph.nodes[i].id : nil })
                .onContinuousHover { phase in switch phase { case .active(let p): hovered = hit(p); case .ended: hovered = -1 } }
                .help(hover >= 0 && graph.nodes.indices.contains(hover) ? "\(graph.nodes[hover].name) · \(graph.nodes[hover].songs) 首 · \(graph.neighbors[hover].count) 位合作者" : "拖动节点，拖空白处平移，滚轮缩放，双击打开档案")

                VStack(alignment: .leading,spacing: 10) {
                    TextField("找一个人",text: $model.graphQuery).textFieldStyle(.roundedBorder)
                    HStack(spacing: 6) { ForEach(["performer","composer","lyricist","arranger"],id: \.self) { role in
                        let on = model.roles.contains(role)
                        Button { if on { if model.roles.count > 1 { model.roles.remove(role) } } else { model.roles.insert(role) } } label: {
                            HStack(spacing: 4) {
                                Circle().fill(on ? graphRoleColor(role) : .secondary.opacity(0.4)).frame(width: 7,height: 7)
                                Text(roleLabels[role] ?? role).font(.caption).foregroundStyle(on ? .primary : .secondary)
                            }
                            .padding(.horizontal,7).padding(.vertical,3)
                            .background(Capsule().fill(on ? graphRoleColor(role).opacity(0.16) : .clear))
                            .overlay(Capsule().strokeBorder(on ? graphRoleColor(role).opacity(0.5) : .secondary.opacity(0.25),lineWidth: 0.8))
                            .contentShape(Capsule())
                        }.buttonStyle(.plain)
                    } }
                    Text("至少一起 \(model.minShared) 首").font(.caption)
                    Slider(value: Binding(get: { Double(model.minShared) },set: { model.minShared = Int64($0) }),in: 1...6,step: 1)
                    Text("\(graph.nodes.count) 人 · \(graph.links.count) 条关系").font(.caption).foregroundStyle(.secondary)
                    if model.around != nil { Button("返回全图") { model.around = nil } }
                    Button { fit(geometry.size) } label: { Label("适合窗口",systemImage: "arrow.up.left.and.arrow.down.right") }.buttonStyle(.borderless)
                    if model.graphBusy { ProgressView().controlSize(.small) }
                }.padding(14).frame(width: 240).glassCard().padding(16)
            }
            .overlay(alignment: .bottomLeading) {
                if let id = model.selected, let person = model.entries.first(where: { $0.id == id }) {
                    HStack(spacing: 12) {
                        CoverImage(path: person.imagePath,size: 44,circle: true,identity: person.name)
                        VStack(alignment: .leading) {
                            Text(person.name).font(.headline); Text(person.roleSummary).font(.caption).foregroundStyle(.secondary)
                            let bands = model.memberships.filter { $0.memberId == id }
                            Text(bands.map { $0.leftAt.isEmpty ? "\($0.bandName) 成员" : "曾属 \($0.bandName)" }.joined(separator: " · ")).font(.caption)
                            HStack { Button("打开档案") { app.show(.person(id)) }; Button("只看附近") { model.around = id } }.buttonStyle(.borderless)
                        }
                    }.padding(14).glassCard().padding(16)
                }
            }
            .task(id: optionsKey) { await model.rebuildGraph(); if !Task.isCancelled { fit(geometry.size) } }
            .onChange(of: geometry.size) { _, size in fit(size) }
        }
        .task(id: key) {
            if let client = app.core {
                await model.load(client,key: key)
                guard !Task.isCancelled else { return }
                var loaded: [Int64:Image] = [:]
                for e in model.entries where !e.imagePath.isEmpty {
                    if let path = try? await client.resolveMedia(e.imagePath),let image = loadImage(atPath: path) { loaded[e.id] = Image(nsImage: image) }
                    if Task.isCancelled { return }
                }; images = loaded
            }
        }
        .onChange(of: model.error) { _, error in if let error { app.errorMessage = error; model.error = nil } }
    }
    private func hit(_ point: CGPoint) -> Int { GraphLayout.nodeAt(model.graph.nodes,x: (point.x-offset.x)/scale,y: (point.y-offset.y)/scale,slop: 4/scale,maxRadius: 21/scale) }
    private func zoom(at point: CGPoint,delta: Double) {
        let next = min(5,max(0.12,scale*exp(-delta*0.0015))), ratio = next/scale
        offset = CGPoint(x: point.x-(point.x-offset.x)*ratio,y: point.y-(point.y-offset.y)*ratio); scale = next
    }
    private func fit(_ size: CGSize) {
        let nodes = model.graph.nodes; guard !nodes.isEmpty else { return }
        let minX = nodes.map { $0.x-$0.r }.min()!, maxX = nodes.map { $0.x+$0.r }.max()!
        let minY = nodes.map { $0.y-$0.r }.min()!, maxY = nodes.map { $0.y+$0.r }.max()!, inset = min(260,size.width*0.26)
        scale = min(3,max(0.15,min((size.width-inset-120)/max(1,maxX-minX),(size.height-120)/max(1,maxY-minY))))
        offset = CGPoint(x: (size.width+inset)/2-(minX+maxX)/2*scale,y: size.height/2-(minY+maxY)/2*scale)
    }
}

func graphRoleColor(_ role: String) -> Color {
    switch role { case "composer": return .blue; case "lyricist": return .orange; case "arranger": return .purple; default: return .pink }
}

/// 只接管图所在区域的滚轮；退出页面即移除监视器。
@MainActor
private struct WheelZoom: NSViewRepresentable {
    var zoom: (CGPoint,Double) -> Void
    func makeCoordinator() -> Coordinator { Coordinator(zoom) }
    func makeNSView(context: Context) -> NSView { let view = NSView(); context.coordinator.view = view; context.coordinator.install(); return view }
    func updateNSView(_ view: NSView,context: Context) { context.coordinator.zoom = zoom }
    static func dismantleNSView(_ view: NSView,coordinator: Coordinator) { coordinator.remove() }
    @MainActor final class Coordinator {
        weak var view: NSView?; var monitor: Any?; var zoom: (CGPoint,Double) -> Void
        init(_ zoom: @escaping (CGPoint,Double) -> Void) { self.zoom = zoom }
        func install() {
            monitor = NSEvent.addLocalMonitorForEvents(matching: .scrollWheel) { [weak self] event in
                MainActor.assumeIsolated {
                    guard let self,let view = self.view,let window = view.window,event.window === window,window.attachedSheet == nil else { return event }
                    let p = view.convert(event.locationInWindow,from: nil)
                    guard view.bounds.contains(p) else { return event }
                    self.zoom(CGPoint(x: p.x,y: view.bounds.height-p.y),Double(event.scrollingDeltaY)); return nil
                }
            }
        }
        func remove() { if let monitor { NSEvent.removeMonitor(monitor) }; monitor = nil }
    }
}
