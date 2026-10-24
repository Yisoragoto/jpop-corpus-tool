import AppKit
import JPCorpusModel
import SwiftUI

@MainActor
struct LyricsWaveform: View {
    @Environment(AppModel.self) private var app
    @Bindable var model: LyricsModel
    let ready: Bool
    @State private var pressed = false
    @State private var drag: (unit: Int, edge: TimingEdge)?

    private var line: TimingDraftLine { model.timing[model.selectedLine] }
    private var range: (from: Double, to: Double) {
        let previous = model.timing.prefix(model.selectedLine).compactMap { Karaoke.lineEnd($0) }.last
        let next = model.timing.dropFirst(model.selectedLine + 1).compactMap { Karaoke.lineStart($0) }.first
        let start = Karaoke.lineStart(line) ?? previous ?? 0
        let end = max(Karaoke.lineEnd(line) ?? start, next ?? start + 4, start + 3)
        return (max(0, start - 1), end + 1)
    }
    var body: some View {
        GeometryReader { geo in
            TimelineView(.animation(minimumInterval: 1.0 / 60, paused: !ready || !app.player.isPlaying)) { _ in
                // Snapshot on the main actor each frame; the renderer and its helpers use only values.
                let visibleRange = range
                let visibleLine = line
                let peaks = model.peaks
                let selectedUnit = model.selectedUnit
                let position = app.player.timingPosition
                let isReady = ready
                Canvas { context, size in
                    let width = size.width, height = size.height
                    func x(_ sec: Double) -> CGFloat { CGFloat((sec - visibleRange.from) / (visibleRange.to - visibleRange.from)) * width }
                    func seconds(_ px: CGFloat) -> Double { visibleRange.from + Double(px / width) * (visibleRange.to - visibleRange.from) }
                    if let peaks, !peaks.isEmpty {
                        for px in stride(from: CGFloat(0), to: width, by: 2) {
                            let a = max(0, min(peaks.count, Int(floor(seconds(px) * 100))))
                            let b = max(a, min(peaks.count, max(a + 1, Int(floor(seconds(px + 2) * 100)))))
                            let peak = peaks[a..<b].max() ?? 0
                            let h = CGFloat(peak) / 255 * (height - 20)
                            context.fill(Path(CGRect(x: px, y: height / 2 - h / 2, width: 1.5, height: max(1, h))), with: .color(.secondary.opacity(0.6)))
                        }
                    }
                    for (ui, unit) in visibleLine.units.enumerated() {
                        guard let start = unit.start else { continue }
                        let x0 = x(start), x1 = unit.end.map { x($0) } ?? x0 + 3
                        context.fill(Path(CGRect(x: x0, y: 0, width: max(2, x1 - x0), height: height)), with: .color(.accentColor.opacity(ui == selectedUnit ? 0.4 : 0.2)))
                        context.fill(Path(CGRect(x: x0, y: 0, width: 1.5, height: height)), with: .color(.accentColor))
                        if unit.end != nil { context.fill(Path(CGRect(x: x1 - 1.5, y: 0, width: 1.5, height: height)), with: .color(.accentColor)) }
                        context.draw(Text(Karaoke.unitText(visibleLine, unit)).font(.system(size: 14)), at: CGPoint(x: x0 + 4, y: 4), anchor: .topLeading)
                    }
                    if isReady && position >= visibleRange.from && position <= visibleRange.to {
                        context.fill(Path(CGRect(x: x(position) - 1, y: 0, width: 2, height: height)), with: .color(.red))
                    }
                }
            }
            .background(.quaternary)
            .clipped()
            .contentShape(Rectangle())
            .gesture(DragGesture(minimumDistance: 0).onChanged { value in
                if !pressed {
                    NSApp.keyWindow?.makeFirstResponder(nil)
                    pressed = true
                    let hit = hit(value.startLocation.x, width: geo.size.width)
                    if let hit {
                        model.select(line: model.selectedLine, unit: hit.unit)
                        if let edge = hit.edge { drag = (hit.unit, edge); model.beginDrag() }
                    } else if ready {
                        let sec = seconds(value.startLocation.x, width: geo.size.width)
                        Task { await app.player.seek(to: max(0, sec)) }
                    }
                }
                if let drag, value.translation.width != 0 {
                    model.setEdge(drag.edge, sec: seconds(value.location.x, width: geo.size.width), record: false)
                }
            }.onEnded { _ in pressed = false; drag = nil })
            .overlay(alignment: .bottomTrailing) {
                if model.peaks == nil { Text("正在读取波形…").font(.caption).padding(6) }
            }
        }
        .frame(height: 120)
        .accessibilityLabel("这一行的波形和每个字的时间")
        .onChange(of: model.selectedLine) { _, _ in pressed = false; drag = nil }
    }
    private func seconds(_ x: CGFloat, width: CGFloat) -> Double { range.from + Double(x / max(1, width)) * (range.to - range.from) }
    private func hit(_ px: CGFloat, width: CGFloat) -> (unit: Int, edge: TimingEdge?)? {
        var best: (unit: Int, edge: TimingEdge, distance: CGFloat)?
        for (ui, u) in line.units.enumerated() {
            for (edge, time) in [(TimingEdge.start, u.start), (.end, u.end)] {
                guard let time else { continue }
                let x = CGFloat((time - range.from) / (range.to - range.from)) * width
                let distance = abs(x - px)
                if distance <= 6 && (best == nil || distance < best!.distance) { best = (ui, edge, distance) }
            }
        }
        if let best { return (best.unit, best.edge) }
        let sec = seconds(px, width: width)
        if let ui = line.units.firstIndex(where: { u in u.start.map { sec >= $0 && sec <= (u.end ?? $0) } ?? false }) { return (ui, nil) }
        return nil
    }
}
