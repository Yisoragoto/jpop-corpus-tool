import AppKit
import JPCore
import JPCorpusModel
import SwiftUI

@MainActor
struct LyricsTimingEditor: View {
    @Environment(AppModel.self) private var app
    @Bindable var model: LyricsModel
    let leave: () -> Void
    @AppStorage("timing-editor-mode") private var mode = "hold"
    @AppStorage("timing-editor-offset-ms") private var offsetMs = 150.0

    private var ready: Bool { app.player.track?.id == model.songId }
    private var duration: Double? { ready ? app.player.track?.durationSec : nil }
    private var statuses: [TimingLineStatus] { model.timing.indices.map { Karaoke.lineStatus(model.timing, $0, duration) } }
    private var current: TimingDraftUnit? { model.validCursor ? model.timing[model.selectedLine].units[model.selectedUnit] : nil }
    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            ScrollView(.horizontal) { HStack {
                Button(ready && app.player.isPlaying ? "暂停" : "播放本首") { toggle() }
                Picker("打点", selection: $model.tapMode) {
                    Text("按住").tag(TimingTapMode.hold); Text("点按").tag(TimingTapMode.tap)
                }.frame(width: 130)
                ForEach([Float(0.5), 0.75, 1], id: \.self) { rate in
                    Button("\(rate, specifier: "%g")×") { app.player.setRate(rate) }
                        .tint(app.player.rate == rate ? .accentColor : .secondary)
                }
                Text("按键提前量")
                TextField("150", value: $model.advanceMilliseconds, format: .number).frame(width: 55)
                Text("ms")
                Spacer()
                Button("撤销") { model.undo() }.disabled(!model.canUndo)
                Button("重做") { model.redo() }.disabled(!model.canRedo)
            } }.frame(height: 34)
            Text("\(statuses.filter { $0.status == "done" }.count) / \(model.timing.filter { !$0.units.isEmpty }.count) 行已逐字")
                .font(.caption).foregroundStyle(.secondary)
            ScrollViewReader { proxy in
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 10) {
                        ForEach(Array(model.timing.enumerated()), id: \.offset) { li, line in
                            HStack(alignment: .top) {
                                Button {
                                    select(line: li, unit: 0); replay(li)
                                } label: {
                                    VStack { Text("第 \(li + 1) 行"); Text(LyricEdit.formatStamp(Karaoke.lineStart(line))) }
                                        .font(.caption.monospacedDigit())
                                }.frame(width: 90)
                                ScrollView(.horizontal) {
                                    HStack(spacing: 4) {
                                        ForEach(Array(line.units.enumerated()), id: \.offset) { ui, unit in
                                            Button { select(line: li, unit: ui) } label: {
                                                Text(Karaoke.unitText(line, unit)).font(.title3).padding(5)
                                            }
                                            .tint(li == model.selectedLine && ui == model.selectedUnit ? Color.accentColor : Color.secondary)
                                            .opacity(unit.start == nil ? 0.5 : 1)
                                            .help("\(LyricEdit.formatStamp(unit.start)) → \(LyricEdit.formatStamp(unit.end))")
                                        }
                                    }
                                }
                                Text(statusLabel(statuses[li].status))
                                    .font(.caption).foregroundStyle(statuses[li].status == "error" ? Color.orange : Color.secondary)
                                    .help(reasonLabel(statuses[li].reason))
                                    .frame(width: 65)
                            }
                            .padding(6)
                            .background(li == model.selectedLine ? Color.accentColor.opacity(0.08) : .clear, in: RoundedRectangle(cornerRadius: 6))
                            .id(li)
                        }
                    }
                }
                .onChange(of: model.selectedLine) { _, line in proxy.scrollTo(line) }
            }
            if model.timing.indices.contains(model.selectedLine) {
                LyricsWaveform(model: model, ready: ready)
            }
            ScrollView(.horizontal) { HStack {
                edgeControl("开始", edge: .start, value: current?.start)
                edgeControl("结尾", edge: .end, value: current?.end)
                Spacer()
                Button("拆开") { model.splitUnit() }.disabled((current.map { $0.charEnd - $0.charStart } ?? 0) < 2)
                Button("和后一个合并") { model.mergeUnit() }
                    .disabled(!model.validCursor || model.selectedUnit + 1 >= (model.timing.indices.contains(model.selectedLine) ? model.timing[model.selectedLine].units.count : 0))
                Button("清空这一行") { model.clearLine() }.disabled(!model.validCursor)
            } }.frame(height: 34)
            Text("空格 打点 · Enter/P 播放暂停 · R 本行重放 · ←→ 选字 · ↑↓ 选行 · [ ] 开始 ±10ms · , . 结尾 ±10ms（Shift ±100ms）· ⌘Z/⇧⌘Z/⌘Y 撤销重做 · ⌘S 保存 · Esc 返回")
                .font(.caption).foregroundStyle(.secondary)
        }
        .disabled(model.busy)
        .background(LyricsKeyboard(action: key))
        .task {
            if !ready { await app.play(songId: model.songId) }
            if let client = app.core { await model.loadPeaks(client) }
        }
        .onAppear {
            model.tapMode = TimingTapMode(rawValue: mode) ?? .hold
            model.advanceMilliseconds = min(400, max(0, offsetMs.isFinite ? offsetMs : 150))
        }
        .onChange(of: model.tapMode) { _, value in mode = value.rawValue }
        .onChange(of: model.advanceMilliseconds) { _, value in
            let bounded = min(400, max(0, value.isFinite ? value : 150))
            offsetMs = bounded
            if bounded != value { model.advanceMilliseconds = bounded }
        }
    }
    private func edgeControl(_ label: String, edge: TimingEdge, value: Double?) -> some View {
        TimingEdgeControl(label: label, value: value, onNudge: { model.nudge(edge, delta: $0) }, onSet: { model.setEdge(edge, sec: $0) })
    }
    private func toggle() { Task { if ready { app.player.toggle() } else { await app.play(songId: model.songId) } } }
    private func select(line: Int, unit: Int) {
        NSApp.keyWindow?.makeFirstResponder(nil)
        model.select(line: line, unit: unit)
    }
    /// 播放器里放的可能已经不是这首（从别处换了歌）：那就先把这首加载到重放点，不在别的歌上跳
    private func replay(_ index: Int) {
        let start = model.replayStart(index)
        Task {
            if ready { await app.player.seek(to: start); app.player.play() }
            else { await app.play(songId: model.songId, at: start) }
        }
    }
    private func save() { Task { if let client = app.core { await model.saveTiming(client) } } }
    private func key(_ event: NSEvent) -> Bool {
        guard !model.busy else { return false }
        if event.type == .keyUp {
            if event.keyCode == 49 { if ready { model.keyUp(at: tapPosition) }; return true }
            return false
        }
        let ctrl = !event.modifierFlags.intersection([.command, .control]).isEmpty
        if event.keyCode == 49 {
            if !event.isARepeat && ready { model.keyDown(at: tapPosition) }
            return true
        }
        if ctrl {
            switch event.keyCode {
            case 6: if event.modifierFlags.contains(.shift) { model.redo() } else { model.undo() }
            case 16: model.redo()
            case 1: save()
            default: return false
            }
            return true
        }
        let step = event.modifierFlags.contains(.shift) ? 0.1 : 0.01
        switch event.keyCode {
        case 36, 76, 35: toggle()
        case 15: model.select(line: model.selectedLine, unit: 0); replay(model.selectedLine)
        case 124: model.moveUnit(1)
        case 123: model.moveUnit(-1)
        case 125: model.moveLine(1)
        case 126: model.moveLine(-1)
        case 33: model.nudge(.start, delta: -step)
        case 30: model.nudge(.start, delta: step)
        case 43: model.nudge(.end, delta: -step)
        case 47: model.nudge(.end, delta: step)
        case 53: leave()
        default: return false
        }
        return true
    }
    private var tapPosition: Double {
        // AVPlayer 在同一进程直接取时钟，无 IPC 往返；慢放时提前量按原曲速率折算。
        app.player.timingPosition - (app.player.isPlaying ? model.advanceMilliseconds / 1000 * Double(app.player.rate) : 0)
    }
    private func statusLabel(_ value: String) -> String {
        ["done": "逐字", "partial": "未完成", "empty": "未打", "error": "时间倒退"][value] ?? value
    }
    private func reasonLabel(_ reason: String?) -> String {
        ["overlapNext": "结尾晚于下一行的开始，播放时这一行整行高亮", "pastEnd": "结尾超出了音频长度", "back": "后面的字比前面的字先唱，存不了"][reason ?? ""] ?? ""
    }
}

@MainActor
private struct TimingEdgeControl: View {
    let label: String
    let value: Double?
    let onNudge: (Double) -> Void
    let onSet: (Double) -> Void
    @State private var draft: String?
    @FocusState private var focused: Bool
    private var formatted: String {
        guard let value else { return "—" }
        return String(format: "%.0f:%06.3f", floor(value / 60), value.truncatingRemainder(dividingBy: 60))
    }
    var body: some View {
        HStack(spacing: 3) {
            Text(label).font(.caption)
            ForEach([-0.1, -0.01], id: \.self) { delta in Button("\(Int(delta * 1000))") { onNudge(delta) } }
            TextField(label, text: Binding(get: { draft ?? formatted }, set: { draft = $0 }))
                .frame(width: 80).focused($focused)
                .onChange(of: focused) { _, active in
                    if active { draft = formatted } else { commit() }
                }
                .onSubmit { focused = false }
                .onKeyPress(.escape) { draft = nil; focused = false; return .handled }
            ForEach([0.01, 0.1], id: \.self) { delta in Button("+\(Int(delta * 1000))") { onNudge(delta) } }
        }.disabled(value == nil)
    }
    private func commit() {
        defer { draft = nil }
        guard let text = draft?.trimmingCharacters(in: .whitespacesAndNewlines),
              text.range(of: #"^(?:([0-9]+):)?([0-9]+(?:\.[0-9]+)?)$"#, options: .regularExpression) != nil else { return }
        let parts = text.split(separator: ":")
        if let seconds = Double(parts.last!), seconds.isFinite {
            let sec = seconds + (parts.count == 2 ? (Double(parts[0]) ?? 0) * 60 : 0)
            if sec.isFinite { onSet(sec) }
        }
    }
}
