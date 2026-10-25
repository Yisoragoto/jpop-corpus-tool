import Foundation
import JPCore

public struct CloudWord: Equatable, Sendable, Identifiable {
    public var lemma: String; public var variants: [String]; public var freq: Int64; public var songCount: Int64
    public var pos: String?; public var mainPos: String; public var surfaces: [String]
    public var id: String { lemma }
    public init(lemma: String, variants: [String] = [], freq: Int64, songCount: Int64 = 1, pos: String? = nil, mainPos: String = "NOUN", surfaces: [String] = []) {
        self.lemma = lemma; self.variants = variants; self.freq = freq; self.songCount = songCount; self.pos = pos; self.mainPos = mainPos; self.surfaces = surfaces
    }
}
public struct PlacedWord: Equatable, Sendable {
    public var word: CloudWord; public var rank: Int; public var x: Double; public var y: Double
    public var w: Double; public var h: Double; public var fontSize: Double; public var vertical: Bool
}
public struct CloudOptions {
    public var width: Double; public var height: Double; public var minFont: Double; public var maxFont: Double
    public var verticalRatio: Double; public var seed: Int; public var measure: (String,Double) -> Double
    public init(width: Double,height: Double,minFont: Double = 12,maxFont: Double = 72,verticalRatio: Double = 0.25,seed: Int = 7,measure: @escaping (String,Double) -> Double) {
        self.width = width; self.height = height; self.minFont = minFont; self.maxFont = maxFont; self.verticalRatio = verticalRatio; self.seed = seed; self.measure = measure
    }
}
public struct SongHits: Identifiable, Sendable {
    public var songId: String; public var title: String; public var artist: String; public var count: Int64; public var first: KwicHit; public var atStart: Bool
    public var id: String { songId }
}
public struct SeededRandom {
    private var state: UInt32
    public init(_ seed: Int) { state = UInt32(truncatingIfNeeded: seed) }
    public mutating func next() -> Double {
        state = state &+ 0x6d2b79f5
        var t = (state ^ (state >> 15)) &* (1 | state)
        t = (t &+ ((t ^ (t >> 7)) &* (61 | t))) ^ t
        return Double(t ^ (t >> 14))/4294967296
    }
}
/// wordCloud.ts，所有文字长度按 Unicode scalar（JS [...text]）。
public enum CloudLayout {
    private static func matches(_ text: String,_ pattern: String) -> Bool { text.range(of: pattern,options: .regularExpression) != nil }
    private static func toHiragana(_ text: String) -> String {
        String(String.UnicodeScalarView(text.unicodeScalars.map { (0x30a1...0x30f6).contains($0.value) ? UnicodeScalar($0.value-0x60)! : $0 }))
    }
    public static func variantKey(_ lemma: String,_ form: LemmaForm?) -> String {
        if matches(lemma,"[A-Za-zＡ-Ｚａ-ｚ]") { return "=\(lemma)" }
        if matches(lemma,"^[ぁ-ゟ゠-ヿー]+$") && lemma.unicodeScalars.count <= 2 { return "k:\(toHiragana(lemma))" }
        guard let f = form, !f.reading.isEmpty else { return "=\(lemma)" }
        return "n:\(f.normalized)|\(f.reading)"
    }
    public static func mergeByLemma(_ rows: [CloudFrequencyRow],limit: Int,keyOf: (String) -> String = { $0 }) -> [CloudWord] {
        struct Acc { var freq: Int64 = 0; var songCount: Int64 = 0; var surfaces: [String] = []; var byLemma: [String:Int64] = [:]; var byPos: [String:Int64] = [:] }
        var groups: [String:Acc] = [:]; var keys: [String] = []
        for r in rows {
            let key = keyOf(r.lemma); if groups[key] == nil { keys.append(key) }; var g = groups[key] ?? Acc()
            g.freq += r.freq; g.songCount = max(g.songCount,r.songCount); g.byLemma[r.lemma,default: 0] += r.freq; g.byPos[r.pos,default: 0] += r.freq
            for sf in r.surfaces where !g.surfaces.contains(sf) { g.surfaces.append(sf) }; groups[key] = g
        }
        func before(_ a: String,_ b: String) -> Bool { a.compare(b,locale: Locale(identifier: "en_US")) == .orderedAscending }
        func byCount(_ m: [String:Int64]) -> [String] { m.keys.sorted { m[$0] == m[$1] ? before($0,$1) : m[$0]! > m[$1]! } }
        return Array(keys.map { key -> CloudWord in
            let g = groups[key]!, variants = byCount(g.byLemma), poses = byCount(g.byPos)
            return CloudWord(lemma: variants.first ?? "",variants: variants,freq: g.freq,songCount: g.songCount,pos: poses.count == 1 ? poses.first : nil,mainPos: poses.first ?? "",surfaces: g.surfaces)
        }.sorted { $0.freq == $1.freq ? before($0.lemma,$1.lemma) : $0.freq > $1.freq }.prefix(max(0,limit)))
    }
    private static let functionNouns: Set<String> = ["こと","事","もの","物","まま","儘","よう","様","とき","ところ","所","ため","為","はず","筈","わけ","訳","うち","ほう","つもり","くらい","ぐらい","ほど","たび","ふり","たち","達","さん","ちゃん","くん","等","など","なんか","みたい","ごと","さぁ","さあ"]
    public static func isFunctionPos(_ pos: String) -> Bool { ["ADP","AUX","SCONJ","CCONJ","PART","DET","PUNCT","SYM","X"].contains(pos) }
    public static func isFunctionWord(_ lemma: String) -> Bool {
        let w = lemma.trimmingCharacters(in: .whitespacesAndNewlines)
        return w.isEmpty || functionNouns.contains(w) || matches(w,"^[ぁ-ゟ゠-ヿー]$") || !matches(w,"[\\p{L}\\p{N}]")
    }
    public static func filterCloudRows(_ rows: [CloudFrequencyRow],hideFunctionWords: Bool,hidden: Set<String>) -> [CloudFrequencyRow] {
        rows.filter { !hidden.contains($0.lemma) && !(hideFunctionWords && (isFunctionWord($0.lemma) || isFunctionPos($0.pos))) }
    }
    public static func fontSizeFor(_ freq: Double,min: Double,max: Double,minFont: Double,maxFont: Double) -> Double {
        let t = max == min ? 1 : (freq-min)/(max-min)
        return floor(minFont+(maxFont-minFont)*pow(Swift.max(0,t),0.55)+0.5)
    }
    public static func layoutCloud(_ words: [CloudWord],_ opt: CloudOptions) -> [PlacedWord] {
        let w = opt.width, h = opt.height
        guard !words.isEmpty, w > 0, h > 0 else { return [] }
        var rand = SeededRandom(opt.seed*31+Int(floor(w+0.5))); var placed: [PlacedWord] = []
        let fmax = Double(words.first!.freq), fmin = Double(words.last!.freq), stretch = max(1,w/h)
        for (rank,word) in words.enumerated() {
            let chars = word.lemma.unicodeScalars.count
            let vertical = rank > 2 && chars >= 2 && chars <= 4 && matches(word.lemma,"^[぀-ヿ㐀-鿿々ー]+$") && rand.next() < opt.verticalRatio
            var size = fontSizeFor(Double(word.freq),min: fmin,max: fmax,minFont: opt.minFont,maxFont: opt.maxFont)
            let start = rand.next()*Double.pi*2; var done = false
            for _ in 0..<4 {
                let ww = vertical ? size*1.1 : opt.measure(word.lemma,size)+size*0.14
                let hh = vertical ? Double(chars)*size+size*0.12 : size*1.18, pad = max(2,size*0.06)
                for step in 0..<2600 {
                    let angle = start+Double(step)*0.17, r = 1.6*(angle-start)
                    let x = w/2+r*cos(angle)*stretch-ww/2, y = h/2+r*sin(angle)-hh/2
                    if x < 4 || y < 4 || x+ww > w-4 || y+hh > h-4 { if r > max(w,h) { break }; continue }
                    if !placed.contains(where: { x < $0.x+$0.w+pad && x+ww+pad > $0.x && y < $0.y+$0.h+pad && y+hh+pad > $0.y }) {
                        placed.append(PlacedWord(word: word,rank: rank,x: x,y: y,w: ww,h: hh,fontSize: size,vertical: vertical)); done = true; break
                    }
                }
                if done { break }; size = max(floor(opt.minFont*0.8+0.5),floor(size*0.78+0.5))
            }
        }
        return placed
    }
    public static func isSentenceStart(_ left: String) -> Bool { left.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }
    public static func groupHitsBySong(_ hits: [KwicHit]) -> [SongHits] {
        var songs: [SongHits] = []; var bySong: [String:Int] = [:]
        for h in hits {
            let n = max(1,h.repeatCount), start = isSentenceStart(h.left)
            if let i = bySong[h.songId] { songs[i].count += n; if start && !songs[i].atStart { songs[i].first = h; songs[i].atStart = true } }
            else { bySong[h.songId] = songs.count; songs.append(SongHits(songId: h.songId,title: h.title,artist: h.artist,count: n,first: h,atStart: start)) }
        }
        return songs.enumerated().sorted {
            if $0.element.atStart != $1.element.atStart { return $0.element.atStart }
            return $0.element.count == $1.element.count ? $0.offset < $1.offset : $0.element.count > $1.element.count
        }.map(\.element)
    }
    public static let posColors: [String:[Double]] = ["NOUN":[159,216,255],"PROPN":[196,170,255],"VERB":[255,160,190],"ADJ":[255,205,120],"ADV":[140,230,170]]
}
