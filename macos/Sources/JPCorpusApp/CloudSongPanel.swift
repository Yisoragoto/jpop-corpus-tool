import JPCore
import JPCorpusModel
import SwiftUI

@MainActor
struct CloudSongPanel: View {
    @Environment(AppModel.self) private var app
    let word: CloudWord
    let hide: () -> Void
    @State private var timings: [WordTiming] = []
    @State private var calibrating: String?
    @State private var lookup = false
    private var model: VisualizationModel { app.visualizations }
    private var scope: Subcorpus {
        SubcorpusScope.mergeSubcorpus(Subcorpus(performerIds: model.cloudFilter.performerIds,yearFrom: model.cloudFilter.yearFrom,yearTo: model.cloudFilter.yearTo),app.scopes.active)
    }
    var body: some View {
        VStack(alignment: .leading,spacing: 10) {
            HStack {
                Text(word.lemma).font(.largeTitle.bold()); Spacer()
                Button { model.picked = nil } label: { Image(systemName: "xmark") }.buttonStyle(.borderless)
            }
            Text("出现 \(model.cloudSongs.reduce(0) { $0+$1.count }) 次 · \(model.cloudSongs.count) 首\(model.cloudTruncated ? "（只查了前 3,000 行）" : "")").font(.caption).foregroundStyle(.secondary)
            Button("不在词云里显示",action: hide).buttonStyle(.borderless)
            ScrollView {
                LazyVStack(alignment: .leading,spacing: 14) {
                    ForEach(model.cloudSongs) { song in songRow(song) }
                }.padding(.vertical,10)
            }
            HStack {
                Button("在检索页看 KWIC") { search() }
                Button("例句和释义") { lookup.toggle() }.popover(isPresented: $lookup) {
                    LookupPane(request: LookupRequest(text: word.lemma,scope: scope),close: { lookup = false }).frame(width: 400,height: 400)
                }
            }.buttonStyle(.borderless).font(.caption)
        }.padding(16).glassCard()
        .task(id: "\(word)|\(scope)|\(app.library.version)") {
            if let core = app.core { await model.loadSongs(core,scope: app.scopes.active); await loadTimings() }
        }
    }
    private func songRow(_ s: SongHits) -> some View {
        let start = WordTimingLogic.wordStart(s.first,timings)
        let isNow = InterfaceLogic.isPlayingSpan(currentSong: app.player.track?.id,playing: app.player.isPlaying,position: app.player.position,songId: s.songId,startSec: start?.sec,endSec: s.first.endSec)
        return VStack(alignment: .leading,spacing: 6) {
            HStack(alignment: .top) {
                Button { play(s,fullscreen: true) } label: { CoverImage(path: app.tracks.first { $0.id == s.songId }?.coverPath ?? "",size: 40,cornerRadius: 6) }.buttonStyle(.plain).help("播放并进入全屏歌词")
                Button { play(s) } label: {
                    VStack(alignment: .leading,spacing: 3) {
                        Text(s.title).fontWeight(.medium); Text("\(s.artist) · \(s.count) 次\(start?.kind == "manual" ? " · 已校准" : "")").font(.caption).foregroundStyle(.secondary)
                        (Text(s.atStart ? "句首 · " : "")+Text(s.first.left)+Text(s.first.keyword).bold().foregroundColor(.accentColor)+Text(s.first.right)).font(.caption).lineLimit(3)
                    }.frame(maxWidth: .infinity,alignment: .leading)
                }.buttonStyle(.plain)
                Button { play(s) } label: { Image(systemName: isNow ? "speaker.wave.2.fill" : "play.fill") }.buttonStyle(.borderless)
            }.contextMenu {
                Button("从行首校准词首") { calibrating = s.songId; Task { await app.player.playFrom(songId: s.songId,utteranceId: s.first.utteranceId,timeSec: max(0,(s.first.timeSec ?? 0)-0.5),leadIn: 0) } }
                Button("歌曲详情") { app.show(.song(s.songId)) }
            }
            if calibrating == s.songId {
                Button("听到「\(s.first.keyword)」时点这里") { calibrate(s) }
                HStack {
                    if start?.kind == "manual" {
                        Button("−0.1") { nudge(s,-0.1) }; Button("+0.1") { nudge(s,0.1) }; Button("重听") { play(s) }
                        Button("撤销") { Task { do { try await app.core?.wordTimeClear(utteranceId: s.first.utteranceId,charStart: WordTimingLogic.keywordSpan(s.first).start); await loadTimings() } catch { app.errorMessage = error.localizedDescription } } }
                    }
                    Button("完成") { calibrating = nil }
                }.buttonStyle(.borderless).font(.caption)
            }
        }
    }
    private func play(_ s: SongHits,fullscreen: Bool = false) {
        Task { await app.player.playFrom(songId: s.songId,utteranceId: s.first.utteranceId,timeSec: WordTimingLogic.wordStart(s.first,timings)?.sec,leadIn: 0); if fullscreen { app.showStage = true } }
    }
    private func loadTimings() async {
        let fresh = (try? await app.core?.wordTimings(model.cloudSongs.map { $0.first.utteranceId })) ?? []
        if !Task.isCancelled { timings = fresh }
    }
    private func calibrate(_ s: SongHits) {
        guard app.player.track?.id == s.songId else { app.errorMessage = "现在放的不是这首，先从这一行放起来"; return }
        set(s,at: max(0,app.player.position-WordTimingLogic.reactionSec))
    }
    private func nudge(_ s: SongHits,_ delta: Double) {
        let start = WordTimingLogic.keywordSpan(s.first).start
        if let manual = timings.first(where: { $0.utteranceId == s.first.utteranceId && $0.source == "manual" && $0.charStart == start }) { set(s,at: max(0,manual.startSec+delta)) }
    }
    private func set(_ s: SongHits,at: Double) {
        let span = WordTimingLogic.keywordSpan(s.first)
        Task { do { try await app.core?.wordTimeSet(utteranceId: s.first.utteranceId,charStart: span.start,charEnd: span.end,startSec: at); await loadTimings(); await app.player.playFrom(songId: s.songId,utteranceId: s.first.utteranceId,timeSec: max(0,at-0.05),leadIn: 0) } catch { app.errorMessage = error.localizedDescription } }
    }
    private func search() {
        app.searchRequest = SearchRequest(keyword: word.variants.joined(separator: " | "),field: .lemma,scope: scope,pos: word.pos); app.section = .search
    }
}
