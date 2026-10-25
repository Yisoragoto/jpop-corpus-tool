import XCTest
import JPCore
@testable import JPCorpusModel

/// Windows corrections.test.ts 的全部 9 例。
final class CorrectionsTests: XCTestCase {
    private let yoru = [TokenEdit("夜"), TokenEdit("が", pos: "ADP"), TokenEdit("明ける", pos: "VERB")]
    func testContiguous() { XCTAssertFalse(Corrections.isContiguous([1])); XCTAssertTrue(Corrections.isContiguous([0,1])); XCTAssertTrue(Corrections.isContiguous([2,0,1])); XCTAssertFalse(Corrections.isContiguous([0,2])); XCTAssertFalse(Corrections.isContiguous([1,1])) }
    func testMergeKeepsHead() { XCTAssertEqual(Corrections.mergeTokens(yoru, [0,1]), [TokenEdit("夜が", lemma: "夜"), yoru[2]]) }
    func testMergeGap() { XCTAssertEqual(Corrections.mergeTokens(yoru, [0,2]), yoru) }
    func testSplit() { XCTAssertEqual(Corrections.splitToken([TokenEdit("明ける", pos: "VERB")], 0, 1), [TokenEdit("明", pos: "VERB"), TokenEdit("ける", pos: "VERB")]) }
    func testCodePoints() { XCTAssertEqual(Corrections.charCount("𠮷野家"), 3); XCTAssertEqual(Corrections.splitToken([TokenEdit("𠮷野家")], 0, 1), [TokenEdit("𠮷"), TokenEdit("野家")]) }
    func testNoEmptyHalf() { let one = [TokenEdit("夜空")]; for (i, at) in [(0,0),(0,2),(5,1)] { XCTAssertEqual(Corrections.splitToken(one, i, at), one) } }
    func testClean() { XCTAssertEqual(Corrections.cleanTokens([TokenEdit(" 夜 ", lemma: "", pos: ""), TokenEdit("   ", lemma: "x"), TokenEdit("が明ける", lemma: "", pos: "VERB")]), [TokenEdit("夜"), TokenEdit("が明ける", pos: "VERB")]) }
    func testEveryField() { XCTAssertTrue(Corrections.sameTokens(yoru, yoru)); XCTAssertFalse(Corrections.sameTokens(yoru, [TokenEdit("夜"), TokenEdit("が", pos: "PART"), yoru[2]])) }
    func testMatchesLine() { XCTAssertTrue(Corrections.matchesLine([TokenEdit("初めて"), TokenEdit("の"), TokenEdit("色")], "初めて の色")); XCTAssertFalse(Corrections.matchesLine([TokenEdit("初めて")], "初めての色")) }
}
final class AvatarIdentityTests: XCTestCase {
    func testWindowsNumericalAndMultiwordCases() { for (name, mark) in [("100回嘔吐","100"),("John Smith","JS"),("ACAね","AC"),("加藤淳人","加藤")] { XCTAssertEqual(AvatarIdentity.monogram(name),mark) } }
    func testUnsignedHash() { XCTAssertEqual(AvatarIdentity.avatarTone("𠮷"), Int(0x20BB7 % 4)); XCTAssertEqual(AvatarIdentity.monogram(""), "♪") }
}
