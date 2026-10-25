import XCTest
import JPCore
@testable import JPCorpusModel

/// wordCloud.test.ts 的全部 21 个用例，输入、断言与 Windows 相同。
final class WordCloudTests: XCTestCase {
    private func row(_ lemma: String,_ pos: String,_ freq: Int64,_ songs: Int64 = 1) -> CloudFrequencyRow { CloudFrequencyRow(lemma: lemma,pos: pos,freq: freq,songCount: songs) }
    private var words: [CloudWord] { (0..<120).map { i in
        let names = ["君","僕","見る","言う","世界","抱きしめる","あなた","love"], chars = Array("空海花夢星")
        return CloudWord(lemma: names[i%8]+String(repeating: String(chars[i%5]),count: i%3),freq: Int64(1000-i*7))
    } }
    private func opts(vertical: Double = 0.25,width: Double = 800) -> CloudOptions { CloudOptions(width: width,height: 420,minFont: 12,maxFont: 72,verticalRatio: vertical) { Double($0.unicodeScalars.count)*$1 } }
    func testSurfacesUnion() {
        let rows = [CloudFrequencyRow(lemma: "見る",pos: "VERB",freq: 9,surfaces: ["見","見る"]),CloudFrequencyRow(lemma: "見る",pos: "AUX",freq: 3,surfaces: ["見","みる"])]
        XCTAssertEqual(CloudLayout.mergeByLemma(rows,limit: 5)[0].surfaces,["見","見る","みる"])
    }
    func testMergeCountsAndMaxSongs() {
        let w = CloudLayout.mergeByLemma([row("好き","ADJ",30,12),row("君","PRON",50,40),row("好き","NOUN",25,9)],limit: 10)
        XCTAssertEqual(w.map(\.lemma),["好き","君"]); XCTAssertEqual(w.map(\.freq),[55,50]); XCTAssertEqual(w.map(\.songCount),[12,40])
    }
    func testOnlyUniquePosSurvives() {
        let w = CloudLayout.mergeByLemma([row("好き","ADJ",30),row("君","PRON",50),row("好き","NOUN",25)],limit: 10)
        XCTAssertEqual(w.first { $0.lemma == "君" }?.pos,"PRON"); XCTAssertNil(w.first { $0.lemma == "好き" }?.pos)
    }
    func testLimitAfterMerging() { XCTAssertEqual(CloudLayout.mergeByLemma([row("a","X",3),row("b","X",2),row("c","X",1)],limit: 2).map(\.lemma),["a","b"]) }
    func testFontExtrema() { XCTAssertEqual(CloudLayout.fontSizeFor(100,min: 1,max: 100,minFont: 12,maxFont: 80),80); XCTAssertEqual(CloudLayout.fontSizeFor(1,min: 1,max: 100,minFont: 12,maxFont: 80),12) }
    func testSingleFrequencyFont() { XCTAssertEqual(CloudLayout.fontSizeFor(5,min: 5,max: 5,minFont: 12,maxFont: 80),80) }
    func testBoxesDoNotOverlapOrEscape() {
        let placed = CloudLayout.layoutCloud(words,opts()); XCTAssertGreaterThan(placed.count,30)
        for p in placed { XCTAssertGreaterThanOrEqual(p.x,0); XCTAssertGreaterThanOrEqual(p.y,0); XCTAssertLessThanOrEqual(p.x+p.w,800); XCTAssertLessThanOrEqual(p.y+p.h,420) }
        for i in placed.indices { for j in placed.indices where j > i {
            let a = placed[i], b = placed[j]; XCTAssertFalse(a.x < b.x+b.w && a.x+a.w > b.x && a.y < b.y+b.h && a.y+a.h > b.y)
        } }
    }
    func testDeterministicLayout() { XCTAssertEqual(CloudLayout.layoutCloud(words,opts()),CloudLayout.layoutCloud(words,opts())) }
    func testTopThreeAndLatinNeverVertical() {
        let placed = CloudLayout.layoutCloud(words,opts(vertical: 1))
        XCTAssertTrue(placed.filter { $0.rank < 3 }.allSatisfy { !$0.vertical }); XCTAssertTrue(placed.filter { $0.word.lemma.contains("love") }.allSatisfy { !$0.vertical })
    }
    func testFirstLargestAndCentered() throws {
        let first = try XCTUnwrap(CloudLayout.layoutCloud(words,opts()).first)
        XCTAssertEqual(first.rank,0); XCTAssertEqual(first.fontSize,72); XCTAssertLessThan(abs(first.x+first.w/2-400),80)
    }
    func testEmptyInputOrCanvas() { XCTAssertEqual(CloudLayout.layoutCloud([],opts()),[]); XCTAssertEqual(CloudLayout.layoutCloud(words,opts(width: 0)),[]) }
    func testSentenceStartSongFirst() throws {
        let songs = CloudLayout.groupHitsBySong(try [VisualFixture.hit("001",utterance: 1,left: "雨が降る"),VisualFixture.hit("001",utterance: 2,left: "雨が降る"),VisualFixture.hit("001",utterance: 3,left: "雨が降る"),VisualFixture.hit("002",utterance: 4,left: "本当なんだ　"),VisualFixture.hit("003",utterance: 5,left: "そして"),VisualFixture.hit("003",utterance: 6)])
        XCTAssertEqual(songs.map(\.songId),["003","001","002"]); XCTAssertEqual(songs.map(\.atStart),[true,false,false]); XCTAssertEqual(songs.map { $0.first.utteranceId },[6,1,4])
    }
    func testSongCountsIncludeCollapsedRepeats() throws {
        let songs = CloudLayout.groupHitsBySong(try [VisualFixture.hit("001",utterance: 10),VisualFixture.hit("002"),VisualFixture.hit("001",repeatCount: 3,utterance: 11),VisualFixture.hit("002")])
        XCTAssertEqual(songs.map(\.songId),["001","002"]); XCTAssertEqual(songs.map(\.count),[4,2])
    }
    func testJumpToFirstHit() throws { XCTAssertEqual(CloudLayout.groupHitsBySong(try [VisualFixture.hit("001",utterance: 10),VisualFixture.hit("001",utterance: 11)])[0].first.utteranceId,10) }
    func testFunctionWordsAndContentWords() {
        for w in ["つ","ら","お","る","ー","こと","まま","もの","よう","~","＂","  "] { XCTAssertTrue(CloudLayout.isFunctionWord(w),w) }
        for w in ["夜","心","言葉","思い出","ヨルシカ","I","月"] { XCTAssertFalse(CloudLayout.isFunctionWord(w),w) }
    }
    func testFunctionPos() { XCTAssertEqual(CloudLayout.filterCloudRows([row("から","ADP",1),row("て","SCONJ",1),row("夜","NOUN",1),row("僕","PRON",1)],hideFunctionWords: true,hidden: []).map(\.lemma),["夜","僕"]) }
    func testHiddenAlwaysRemoved() {
        let rows = [row("夜","",1),row("つ","",1),row("花","",1)]
        XCTAssertEqual(CloudLayout.filterCloudRows(rows,hideFunctionWords: true,hidden: ["花"]).map(\.lemma),["夜"])
        XCTAssertEqual(CloudLayout.filterCloudRows(rows,hideFunctionWords: false,hidden: ["花"]).map(\.lemma),["夜","つ"])
    }
    private func forms() throws -> [String:LemmaForm] {
        let values = [["思い出","思い出","おもいで"],["想い出","思い出","おもいで"],["遠く","遠い","とおく"],["遠い","遠い","とおい"],["Right","ライト","らいと"],["ライト","ライト","らいと"],["かみ","紙","かみ"],["紙","紙","かみ"],["ひとり","一人","ひとり"],["一人","一人","ひとり"]]
        return try Dictionary(uniqueKeysWithValues: values.map { ($0[0],try VisualFixture.decode(LemmaForm.self,["lemma":$0[0],"normalized":$0[1],"reading":$0[2]])) })
    }
    func testVariantNormalizationAndReadingMustAgree() throws {
        let f = try forms(); func key(_ l: String) -> String { CloudLayout.variantKey(l,f[l]) }
        XCTAssertEqual(key("想い出"),key("思い出")); XCTAssertEqual(key("ひとり"),key("一人")); XCTAssertNotEqual(key("遠く"),key("遠い"))
    }
    func testLatinAndShortKanaStaySeparate() throws {
        let f = try forms(); XCTAssertNotEqual(CloudLayout.variantKey("Right",f["Right"]),CloudLayout.variantKey("ライト",f["ライト"]))
        XCTAssertNotEqual(CloudLayout.variantKey("かみ",f["かみ"]),CloudLayout.variantKey("紙",f["紙"])); XCTAssertEqual(CloudLayout.variantKey("ヒト",nil),CloudLayout.variantKey("ひと",nil))
    }
    func testMostFrequentVariantDisplayedAndAllKept() throws {
        let f = try forms(), words = CloudLayout.mergeByLemma([row("想い出","NOUN",21,5),row("思い出","NOUN",52,20),row("夜","NOUN",60,30)],limit: 10) { CloudLayout.variantKey($0,f[$0]) }
        XCTAssertEqual(words.map(\.lemma),["思い出","夜"]); XCTAssertEqual(words.map(\.freq),[73,60]); XCTAssertEqual(words.map(\.variants),[["思い出","想い出"],["夜"]]); XCTAssertEqual(words[0].mainPos,"NOUN")
    }
    func testSentenceStartIsOnlyBlankLeft() {
        for s in ["","  ","　"] { XCTAssertTrue(CloudLayout.isSentenceStart(s)) }; for s in ["本当なんだ　","僕は今 ","終わり。","雨が"] { XCTAssertFalse(CloudLayout.isSentenceStart(s)) }
    }
    func testWindowsPlacedCoordinates() {
        let placed = CloudLayout.layoutCloud(words,opts()); XCTAssertEqual(placed.count,38)
        let expected = [[358.96,167.52,82.08,84.96,72],[446.70309231252935,147.43187581437454,154.08,84.96,72],[289.6319287143865,258.2934803315741,293.94,83.78,71],[189.48838090249038,158.65768322699057,151.94,83.78,71],[209.03480373726492,62.9360169425299,222.94,83.78,71],[256.20331659070723,349.42964159487957,392.7,64.89999999999999,55],[448.1087097726693,55.486825927076865,219.8,82.6,70],[63.180141639791444,266.75277839598317,221.02,50.739999999999995,43]]
        for i in expected.indices { let p = placed[i]; for (a,b) in zip([p.x,p.y,p.w,p.h,p.fontSize],expected[i]) { XCTAssertEqual(a,b,accuracy: 1e-8) }; XCTAssertFalse(p.vertical) }
    }
}
