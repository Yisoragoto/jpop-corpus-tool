import JPCore
import JPCorpusModel
import SwiftUI

/// 循环区间的调整栏（#64），和 Windows 的 LoopAdjustBar 一样：时间轴开始晚了、结束早了，循环时听不全，在这里挪一挪。
/// 起点、终点各有 −0.5 / −0.1 / +0.1 / +0.5 秒，改了马上生效；改起点会从新起点重放。
/// 键盘：`[` `]` 挪起点，⇧`[` `]` 挪终点，按着 Option 一下 0.5 秒（见 PlaybackKeyboard）。
/// 调过的按行记住，试听和制卡截的那一段也用调过的。
@MainActor
struct LoopAdjustBar: View {
    @Environment(AppModel.self) private var app
    private static let steps = [-PlayerModel.coarseLoopStepMs, -PlayerModel.fineLoopStepMs,
                                PlayerModel.fineLoopStepMs, PlayerModel.coarseLoopStepMs]

    var body: some View {
        let player = app.player
        if let region = player.loopRegion {
            let timed = player.timedLineIds
            let first = timed.firstIndex(of: region.firstUtteranceId)
            let last = timed.firstIndex(of: region.lastUtteranceId)
            let count = player.loopLineCount
            VStack(alignment: .leading, spacing: 6) {
                HStack(spacing: 10) {
                    Label(count > 1 ? "循环 \(count) 行" : "循环这一句", systemImage: "repeat")
                        .font(.app(.callout).weight(.semibold))
                        .foregroundStyle(.tint)
                    Spacer(minLength: 4)
                    Button("取消循环") { Task { await player.toggleLoop(nil) } }
                        .buttonStyle(.link)
                        .help("⌘L")
                }
                edge(start: true, region: region)
                edge(start: false, region: region)
                HStack(spacing: 12) {
                    Button("往前加一行") { Task { await player.extendLoop(before: true) } }
                        .disabled((first ?? 0) <= 0)
                    Button("往后加一行") { Task { await player.extendLoop(before: false) } }
                        .disabled(last.map { $0 >= timed.count - 1 } ?? true)
                    Button("重置") { Task { await player.resetLoopAdjust() } }
                        .disabled(!region.adjusted)
                        .help("回到按时间轴算的起止")
                }
                .buttonStyle(.link)
                .font(.app(.caption))
            }
            .padding(10)
            .background(RoundedRectangle(cornerRadius: 10, style: .continuous).fill(.quaternary.opacity(0.6)))
            .accessibilityElement(children: .contain)
            .accessibilityLabel(Text("循环区间"))
        }
    }

    private func edge(start: Bool, region: LoopRegion) -> some View {
        let sec = start ? region.startSec : region.endSec
        let delta = start ? region.startDeltaMs : region.endDeltaMs
        let keys = start ? "[ ]" : "⇧[ ]"
        return HStack(spacing: 4) {
            Text(start ? "起点" : "终点")
                .font(.app(.caption))
                .foregroundStyle(.secondary)
                .frame(minWidth: 30, alignment: .leading)
            ForEach(Self.steps.prefix(2), id: \.self) { ms in stepButton(ms, start: start, keys: keys) }
            HStack(alignment: .firstTextBaseline, spacing: 2) {
                Text(verbatim: LoopFormat.time(sec)).font(.app(.callout).monospacedDigit())
                if delta != 0 {
                    Text(verbatim: LoopFormat.delta(delta)).font(.app(.caption2).monospacedDigit()).foregroundStyle(.tint)
                }
            }
            .frame(minWidth: 74)
            ForEach(Self.steps.suffix(2), id: \.self) { ms in stepButton(ms, start: start, keys: keys) }
        }
        .accessibilityElement(children: .contain)
        .accessibilityLabel(Text(start ? "起点" : "终点"))
    }

    private func stepButton(_ ms: Int, start: Bool, keys: String) -> some View {
        let player = app.player
        let sec = String(format: "%g", Double(abs(ms)) / 1000)
        return Button {
            Task {
                if start { await player.nudgeLoop(startMs: ms) } else { await player.nudgeLoop(endMs: ms) }
            }
        } label: {
            Text(verbatim: LoopFormat.step(ms)).font(.app(.caption).monospacedDigit()).frame(minWidth: 30)
        }
        .buttonStyle(.bordered)
        .controlSize(.small)
        .help(ms < 0 ? "提前 \(sec) 秒（\(keys)）" : "推后 \(sec) 秒（\(keys)）")
    }
}
