import Foundation
import JPCore
import Observation

public protocol AnkiCalling: AnyObject, Sendable {
    func ankiStatus() async throws -> AnkiStatus
    func ankiWords(pos: [String], skipStudied: Bool, minCount: Int64, songIds: [String], limit: Int) async throws -> [WordCandidate]
    func ankiPreview(lemma: String, pos: String, maxExamples: Int, maxDicts: Int?) async throws -> AnkiCard
    func ankiExportStart(_ request: AnkiBatchRequest) async throws -> Int
    func ankiUpdateStart(_ request: AnkiBatchRequest) async throws -> Int
    func ankiRefreshPreview(deck: String, scope: AnkiRefreshScope) async throws -> Int
    func ankiRefreshStart(deck: String, scope: AnkiRefreshScope) async throws
    func ankiIsRunning() async throws -> Bool
    func ankiCancel() async throws
    func ankiProgress() async throws -> JobProgress<AnkiJobResult>
    func ankiMiningReport(now: String, deckContains: String?) async throws -> MiningReportResult
}
extension CorpusClient: AnkiCalling {}

public enum AnkiLogic {
    public static let posChoices = ["NOUN", "PROPN", "VERB", "ADJ", "ADV"]
    public static let outcomeLabels = ["added":"新增", "updated":"追加例句", "alreadyComplete":"已是最新", "skipped":"已有，跳过", "noExamples":"无例句", "failed":"失败", "refreshed":"已刷新", "notFound":"Anki 里没有"]
    // AnkiPage.tsx startedProgress、默认牌组、计数与选择。
    public static func startedProgress(_ job: String, total: Int) -> AnkiProgress { AnkiProgress(job: job, total: total) }
    public static func defaultDeck(_ decks: [String]) -> String {
        decks.first { $0.range(of: "日本|jp|japanese|語", options: [.regularExpression, .caseInsensitive]) != nil } ?? decks.first ?? ""
    }
    public static func counts(_ log: [AnkiProgress]) -> [String: Int] { log.reduce(into: [:]) { $0[$1.outcome, default: 0] += 1 } }
    public static func percent(_ progress: AnkiProgress?) -> Double { guard let p = progress, p.total > 0 else { return 0 }; return Double(p.done)/Double(p.total)*100 }
    public static func selected(_ words: [WordCandidate], picked: Set<String>) -> [AnkiExportItem] { words.filter { picked.contains($0.lemma) }.map { AnkiExportItem(lemma: $0.lemma, pos: $0.pos) } }
    public static func toggle(_ picked: Set<String>, lemma: String) -> Set<String> { var next = picked; if !next.insert(lemma).inserted { next.remove(lemma) }; return next }
    public static func artistSubdeck(_ parent: String, artist: String) -> String? {
        let parent = parent.trimmingCharacters(in: .whitespacesAndNewlines)
        var name = String(artist.split(separator: "/", omittingEmptySubsequences: false).first ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
        while name.contains("::") { name = name.replacingOccurrences(of: "::", with: ":") }
        name = name.trimmingCharacters(in: .whitespacesAndNewlines)
        return name.isEmpty || parent.isEmpty ? nil : "\(parent)::\(name)"
    }
    // MiningReportCard.tsx localTimestamp。
    public static func localTimestamp(_ date: Date = Date(), timeZone: TimeZone = .current) -> String {
        let f = DateFormatter(); f.locale = Locale(identifier: "en_US_POSIX"); f.calendar = Calendar(identifier: .gregorian)
        f.timeZone = timeZone; f.dateFormat = "yyyy-MM-dd HH:mm"; return f.string(from: date)
    }
}

@MainActor @Observable
public final class AnkiModel {
    public var status: AnkiStatus?
    public var words: [WordCandidate] = []
    public var picked: Set<String> = []
    public var options = AnkiBatchRequest() { didSet { if options.deck != oldValue.deck { refreshCount = nil; countRevision += 1 } } }
    public var pos = AnkiLogic.posChoices
    public var skipStudied = true
    public var preview: AnkiCard?
    public var progress: AnkiProgress?
    public var log: [AnkiProgress] = []
    public var running = false
    public var busy = false
    public var counting = false
    public var refreshScope: AnkiRefreshScope = .deck { didSet { refreshCount = nil; countRevision += 1 } }
    public var refreshCount: Int?
    public var report: MiningReportResult?
    public var reporting = false
    public var error: String?
    private var wordRevision = 0
    private var countRevision = 0
    public init() {}
    public func load(_ client: any AnkiCalling) async {
        if let value = try? await client.ankiStatus() { status = value; options.deck = AnkiLogic.defaultDeck(value.decks) }
        running = (try? await client.ankiIsRunning()) ?? false
        await loadWords(client)
    }
    public func loadWords(_ client: any AnkiCalling) async {
        wordRevision += 1; let revision = wordRevision
        do {
            let fresh = try await client.ankiWords(pos: pos, skipStudied: skipStudied, minCount: 1, songIds: [], limit: 300)
            if revision == wordRevision && !Task.isCancelled { words = fresh }
        } catch { if revision == wordRevision && !Task.isCancelled { self.error = error.localizedDescription } }
    }
    public func showPreview(_ word: WordCandidate, client: any AnkiCalling) async {
        do { preview = try await client.ankiPreview(lemma: word.lemma, pos: word.pos, maxExamples: options.maxExamples, maxDicts: options.maxDicts) }
        catch { self.error = error.localizedDescription }
    }
    public func start(_ client: any AnkiCalling, update: Bool = false) async {
        var request = options; request.words = AnkiLogic.selected(words, picked: picked)
        guard !request.words.isEmpty else { error = "先勾几个词"; return }
        busy = true; log = []; defer { busy = false }
        do {
            let total: Int
            if update { total = try await client.ankiUpdateStart(request) }
            else { total = try await client.ankiExportStart(request) }
            progress = AnkiLogic.startedProgress(update ? "update" : "export", total: total); running = true
        } catch { self.error = error.localizedDescription }
    }
    public func countRefresh(_ client: any AnkiCalling) async {
        counting = true; let revision = countRevision; defer { counting = false }
        do { let count = try await client.ankiRefreshPreview(deck: options.deck, scope: refreshScope); if revision == countRevision { refreshCount = count } }
        catch { if revision == countRevision { self.error = error.localizedDescription } }
    }
    public func startRefresh(_ client: any AnkiCalling) async {
        busy = true; log = []; defer { busy = false }
        do {
            try await client.ankiRefreshStart(deck: options.deck, scope: refreshScope)
            progress = AnkiLogic.startedProgress("refresh", total: refreshCount ?? 0); refreshCount = nil; running = true
        } catch { self.error = error.localizedDescription }
    }
    public func cancel(_ client: any AnkiCalling) async { do { try await client.ankiCancel() } catch { self.error = error.localizedDescription } }
    public func poll(_ client: any AnkiCalling) async {
        do {
            let view = try await client.ankiProgress()
            if let result = view.result { progress = result.progress; log = result.log }
            if view.finished {
                if let message = view.error { error = message }
                else if let p = progress, !p.error.isEmpty { error = p.error }
                status = (try? await client.ankiStatus()) ?? status
                await loadWords(client)
            }
            running = view.running
        } catch { self.error = error.localizedDescription; running = false }
    }
    public func generateReport(_ client: any AnkiCalling) async {
        reporting = true; defer { reporting = false }
        do { report = try await client.ankiMiningReport(now: AnkiLogic.localTimestamp(), deckContains: nil) }
        catch { report = nil; self.error = error.localizedDescription }
    }
}
