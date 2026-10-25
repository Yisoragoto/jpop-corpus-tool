import XCTest
@testable import JPCore
@testable import JPCorpusModel

/// Windows newSongs.test.ts 的全部 18 例。
final class NewSongsTests: XCTestCase {
    private func item(path: String = "D:\\music\\ヨルシカ\\晴る.flac", title: String = "晴る", artist: String = "ヨルシカ", source: String = "tag", warning: String? = nil, kind: PlanAction.Kind = .new) throws -> PlanItem {
        let data = try JSONSerialization.data(withJSONObject: ["path":path,"fileName":"晴る.flac","title":title,"artist":artist,"album":"","durationSec":200,"hasLyrics":false,"warning":warning as Any? ?? NSNull(),"artistSource":source,"action":["kind":kind.rawValue,"songId":"211"]])
        return try JSONDecoder().decode(PlanItem.self,from:data)
    }
    private func folder(_ path: String) throws -> WatchedFolder { try decoded(WatchedFolder.self, "{\"path\":\"\(path)\",\"reason\":\"libraryAudio\",\"depth\":1,\"songs\":1,\"unknown\":0,\"new\":0}") }
    private func folders() throws -> [WatchedFolder] { try [folder("D:/music"),folder("D:/music/ヨルシカ"),folder("E:/cloud music/sakana")] }
    func testTagged() throws { XCTAssertNil(NewSongs.reviewReason(try [item(),item(source:"fileName")])) }
    func testUnreadable() throws { XCTAssertTrue(NewSongs.reviewReason(try [item(),item(warning:"读不出元数据")])!.contains("1 首读不出标签")) }
    func testFolderGuess() throws { XCTAssertTrue(NewSongs.reviewReason(try [item(artist:"MyMusic",source:"folder")])!.contains("文件夹名猜的")) }
    func testAnonymous() throws { XCTAssertTrue(NewSongs.reviewReason(try [item(artist:"",source:"none")])!.contains("定不出歌手")) }
    func testLimit() throws { let many = try (0...20).map { try item(path:"D:/m/\($0).flac") }; XCTAssertNil(NewSongs.reviewReason(Array(many.prefix(20)))); XCTAssertTrue(NewSongs.reviewReason(many)!.contains("21 首")) }
    func testNewOnly() throws { let scan = ScanResult(summary: PlanSummary(total:3,new:1,alreadyImported:0,possibleDuplicates:1,duplicatesInBatch:0,skipped:1),items:try [item(),item(kind:.possibleDuplicate),item(kind:.skipped)],tokenizerReady:true,ignored:[]); XCTAssertEqual(NewSongs.newItems(scan).map { $0.action.kind },[.new]) }
    func testDescriptionLimit() throws { let three = try [item(),item(title:"左右盲"),item(title:"怪獣",artist:"サカナクション")]; XCTAssertEqual(NewSongs.describeNewSongs(three),"晴る — ヨルシカ、左右盲 — ヨルシカ、怪獣 — サカナクション"); XCTAssertEqual(NewSongs.describeNewSongs(three + [try item(title:"モス")]),"晴る — ヨルシカ、左右盲 — ヨルシカ、怪獣 — サカナクション 等 4 首") }
    func testAnonymousDescription() throws { XCTAssertEqual(NewSongs.describeNewSongs(try [item(artist:" ")]),"晴る") }
    func testNearestFolder() throws { let fs = try folders(); XCTAssertEqual(NewSongs.folderOf(fs,"D:/music/ヨルシカ/晴る.flac")?.path,"D:/music/ヨルシカ"); XCTAssertEqual(NewSongs.folderOf(fs,"D:/music/Vaundy/怪獣の花唄.flac")?.path,"D:/music") }
    func testCaseAndSlashes() throws { XCTAssertEqual(NewSongs.folderOf(try folders(),"e:/Cloud Music/sakana/a.flac")?.path,"E:/cloud music/sakana") }
    func testBoundary() throws { XCTAssertNil(NewSongs.folderOf(try folders(),"D:/music2/a.flac")) }
    func testFolderCounts() throws {
        let found = NewAudio(folders:try folders(),scan:ScanResult(summary:PlanSummary(total:0,new:0,alreadyImported:0,possibleDuplicates:0,duplicatesInBatch:0,skipped:0),items:[],tokenizerReady:true,ignored:[]))
        XCTAssertEqual(NewSongs.foldersOf(found,try [item(path:"E:/cloud music/sakana/a.flac"),item(path:"D:/music/ヨルシカ/b.flac"),item(path:"D:/music/ヨルシカ/c.flac")]),["D:/music/ヨルシカ","E:/cloud music/sakana"])
    }
    private func report() throws -> ImportResult { try decoded(ImportResult.self,#"{"cancelled":false,"imported":3,"failed":1,"lyricLines":30,"tokens":200,"tracks":[{"songId":"211","path":"D:/m/a.flac","title":"晴る","outcome":{"kind":"imported","lyricLines":30}},{"songId":"212","path":"d:/m/b.flac","title":"左右盲","outcome":{"kind":"imported","lyricLines":0}},{"songId":"213","path":"D:/m/c.flac","title":"怪獣","outcome":{"kind":"imported","lyricLines":0}},{"songId":"214","path":"D:/m/d.flac","title":"","outcome":{"kind":"failed","error":"磁盘满了"}}]}"#) }
    private func planned() throws -> [PlanItem] { try [item(path:"D:/m/a.flac"),item(path:"D:/m/b.flac",title:"左右盲"),item(path:"D:/m/c.flac",title:"怪獣",artist:"サカナクション"),item(path:"D:/m/d.flac",title:"壊れた曲",artist:"誰か")] }
    func testSummary() throws { let r = NewSongs.summarizeImport(try report(),try planned()); XCTAssertEqual(r.imported,3); XCTAssertEqual(r.artists,["ヨルシカ","サカナクション"]); XCTAssertEqual(r.withoutLyrics,["212","213"]); XCTAssertEqual(r.failed,[.init(title:"D:/m/d.flac",error:"磁盘满了")]); XCTAssertFalse(r.cancelled) }
    func testFailedArtist() throws { XCTAssertFalse(NewSongs.summarizeImport(try report(),try planned()).artists.contains("誰か")) }
    func testMalformedIgnores() { for raw: String? in [nil,"","不是 JSON","null","[]",#"{"folders":"D:\\m","files":[1,null,""]}"#] { XCTAssertEqual(NewSongs.parseIgnores(raw),.init()) }; XCTAssertEqual(NewSongs.parseIgnores(#"{"folders":["D:\\m"],"files":["D:\\m\\a.flac",7]}"#),.init(folders:["D:\\m"],files:["D:\\m\\a.flac"])) }
    func testIgnoresDeduplicate() { let once = NewSongs.withIgnored(.init(),.init(folders:["D:\\m"],files:["D:\\m\\a.flac"])); XCTAssertEqual(NewSongs.withIgnored(once,.init(folders:["D:/m/"],files:["d:/M/A.flac","D:\\m\\b.flac"])),.init(folders:["D:\\m"],files:["D:\\m\\a.flac","D:\\m\\b.flac"])) }
    func testRestoreFolder() { XCTAssertEqual(NewSongs.withoutIgnoredFolder(.init(folders:["D:\\m","E:\\other"],files:["D:\\m\\a.flac"]),"d:/m/"),.init(folders:["E:\\other"],files:["D:\\m\\a.flac"])) }
    func testPathKey() { XCTAssertEqual(NewSongs.pathKey("D:\\Music\\ヨルシカ\\"),"d:/music/ヨルシカ"); XCTAssertEqual(NewSongs.pathKey("d:/music/ヨルシカ"),"d:/music/ヨルシカ") }
}
