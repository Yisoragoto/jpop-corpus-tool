import Foundation
import JPCorpusModel
import JPCore
import SwiftUI

@MainActor
struct CollocationNetworkView: View {
    @Environment(AppModel.self) private var app
    let query: NetworkQuery
    let onPick: (String) -> Void
    @State private var measure: CollocationMeasure = .logDice
    @State private var contentOnly = true
    @State private var result: NetworkResult?
    @State private var layout: CollocationNetworkLayout?
    @State private var errorMessage = ""

    private var request: NetworkQuery {
        var value = query
        value.collocation.measure = measure
        value.collocation.contentOnly = contentOnly
        return value
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                controls
                if !errorMessage.isEmpty {
                    Text(errorMessage).foregroundStyle(.red).textSelection(.enabled)
                } else if let result, let layout {
                    if result.nodes.isEmpty {
                        Text("没有共现 2 次以上的词").foregroundStyle(.secondary)
                    } else {
                        graph(result, layout: layout)
                    }
                } else {
                    ProgressView("计算中…").controlSize(.small)
                }
            }
            .padding(20)
        }
        .task(id: request) { await load() }
    }

    private var controls: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("搭配网络").font(.headline)
            HStack {
                Picker("搭配强度", selection: $measure) {
                    Text("logDice").tag(CollocationMeasure.logDice)
                    Text("MI").tag(CollocationMeasure.mi)
                    Text("LL").tag(CollocationMeasure.logLikelihood)
                    Text("次数").tag(CollocationMeasure.frequency)
                }
                .pickerStyle(.segmented).fixedSize()
                Toggle("只看实词", isOn: $contentOnly).controlSize(.small)
                Spacer()
            }
        }
    }

    private func graph(_ result: NetworkResult, layout: CollocationNetworkLayout) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("「\(result.node)」（\(result.nodeFreq) 次）和它最强的 \(result.nodes.count) 个搭配词：越靠中间越强，圆点大小是总频次；灰线是搭配词彼此之间常一起出现（左右 3 词内）。点一个词检索该词。")
                .font(.caption).foregroundStyle(.secondary)
            CollocationNetworkCanvas(result: result, layout: layout, onPick: onPick)
                .aspectRatio(760.0 / 560.0, contentMode: .fit)
                .frame(minHeight: 340)
            Text("\(result.provenance.scope) · \(result.provenance.songs) 首 / \(result.provenance.tokens) 词（\(result.provenance.unit == .distinctLines ? "重复行只算一次" : "全部词次")）")
                .font(.caption2).foregroundStyle(.secondary)
        }
    }

    private func load() async {
        guard !Task.isCancelled else { return }
        result = nil
        layout = nil
        errorMessage = ""
        guard let client = app.core else { return }
        let value = request
        do {
            let found = try await client.collocationNetwork(value)
            guard !Task.isCancelled else { return }
            // 固定布局只随结果计算一次，Canvas 只负责缩放和绘制。
            let positions = await Task.detached(priority: .userInitiated) {
                CollocationNetworkLayout.make(found)
            }.value
            guard !Task.isCancelled else { return }
            result = found
            layout = positions
        } catch {
            guard !Task.isCancelled else { return }
            errorMessage = error.localizedDescription
        }
    }
}

private struct CollocationNetworkLayout: Sendable {
    struct Point: Sendable {
        let x: Double
        let y: Double
        let radius: Double
        var position: CGPoint { CGPoint(x: x, y: y) }
    }

    let points: [Point]
    let edgeWidths: [Double]
    let spokeWidths: [Double]

    static func make(_ result: NetworkResult) -> Self {
        let high = max(0, result.nodes.map(\.score).max() ?? 0)
        let low = min(0, result.nodes.map(\.score).min() ?? 0)
        let maxFrequency = Double(max(1, result.nodes.map(\.freq).max() ?? 1))
        let count = result.nodes.count
        let points = result.nodes.enumerated().map { index, node -> Point in
            let strength = high > low ? (high - node.score) / (high - low) : 0.5
            let distance = 150 + strength * 100
            let slot = index.isMultiple(of: 2) ? index / 2 : count - 1 - (index - 1) / 2
            let angle = Double(slot) / Double(max(1, count)) * .pi * 2 - .pi / 2
            return Point(x: 380 + distance * cos(angle), y: 280 + distance * sin(angle) * 0.82,
                         radius: 4 + 10 * sqrt(Double(max(0, node.freq)) / maxFrequency))
        }
        let maxEdge = max(1, result.edges.map(\.logDice).max() ?? 1)
        let widths = result.edges.map { 1 + 3 * max(0, $0.logDice) / maxEdge }
        let spokes = result.nodes.map { node -> Double in
            let strength = high > low ? (node.score - low) / (high - low) : 0.5
            return 0.5 + max(0, min(1, strength)) * 1.5
        }
        return Self(points: points, edgeWidths: widths, spokeWidths: spokes)
    }
}

private struct CollocationNetworkCanvas: View {
    let result: NetworkResult
    let layout: CollocationNetworkLayout
    let onPick: (String) -> Void

    var body: some View {
        GeometryReader { geometry in
            ZStack(alignment: .topLeading) {
                Canvas { context, size in
                    context.scaleBy(x: size.width / 760, y: size.height / 560)
                    drawEdges(in: &context)
                    drawNodes(in: &context)
                }
                .accessibilityHidden(true)
                ForEach(result.nodes.indices, id: \.self) { index in
                    nodeButton(index, size: geometry.size)
                }
            }
        }
        .accessibilityElement(children: .contain)
        .accessibilityLabel("「\(result.node)」的搭配网络")
    }

    private func nodeButton(_ index: Int, size: CGSize) -> some View {
        let point = layout.points[index]
        let node = result.nodes[index]
        return Button { onPick(node.lemma) } label: {
            Color.clear
                .frame(width: max(64, CGFloat(node.lemma.count * 14)), height: 52)
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .position(x: point.x * size.width / 760, y: (point.y - 10) * size.height / 560)
        .help("\(node.lemma)（\(node.pos)）· 范围内 \(node.freq) 次 · 共现 \(node.cofreq) 次 · logDice \(String(format: "%.2f", node.logDice))")
        .accessibilityLabel("检索「\(node.lemma)」，共现 \(node.cofreq) 次")
    }

    private func drawEdges(in context: inout GraphicsContext) {
        for index in result.edges.indices {
            let edge = result.edges[index]
            guard layout.points.indices.contains(edge.a), layout.points.indices.contains(edge.b) else { continue }
            var path = Path()
            path.move(to: layout.points[edge.a].position)
            path.addLine(to: layout.points[edge.b].position)
            context.stroke(path, with: .color(.secondary.opacity(0.35)), lineWidth: layout.edgeWidths[index])
        }
        for index in layout.points.indices {
            var path = Path()
            path.move(to: CGPoint(x: 380, y: 280))
            path.addLine(to: layout.points[index].position)
            context.stroke(path, with: .color(.accentColor.opacity(0.18)), lineWidth: layout.spokeWidths[index])
        }
    }

    private func drawNodes(in context: inout GraphicsContext) {
        context.fill(Path(ellipseIn: CGRect(x: 358, y: 258, width: 44, height: 44)),
                     with: .color(.accentColor.opacity(0.25)))
        context.draw(Text(result.node).font(.headline), at: CGPoint(x: 380, y: 280))
        for index in layout.points.indices {
            let point = layout.points[index]
            let circle = CGRect(x: point.x - point.radius, y: point.y - point.radius,
                                width: point.radius * 2, height: point.radius * 2)
            context.fill(Path(ellipseIn: circle), with: .color(.accentColor.opacity(0.8)))
            context.draw(Text(result.nodes[index].lemma).font(.system(size: 13)),
                         at: CGPoint(x: point.x, y: point.y - point.radius - 12))
        }
    }
}
