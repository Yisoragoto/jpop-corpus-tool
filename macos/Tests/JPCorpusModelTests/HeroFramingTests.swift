import XCTest
@testable import JPCorpusModel

final class HeroFramingTests: XCTestCase {
    /// 竖拍照片铺进横幅：脸在照片 70% 高处时，往上挪到横幅 40% 处
    func testPortraitPhotoFollowsFace() {
        let placed = HeroFraming.place(image: CGSize(width: 1000, height: 1500), in: CGSize(width: 1000, height: 380), focus: CGPoint(x: 0.5, y: 0.7))
        XCTAssertEqual(placed.size.width, 1000, accuracy: 1e-9)
        XCTAssertEqual(placed.size.height, 1500, accuracy: 1e-9)
        XCTAssertEqual(placed.origin.x, 0, accuracy: 1e-9)
        XCTAssertEqual(placed.origin.y, 0.4 * 380 - 0.7 * 1500, accuracy: 1e-9)
    }
    /// 不会挪出照片边缘：脸贴着最上沿也只能顶到 0
    func testNeverShowsEmptyEdges() {
        let top = HeroFraming.place(image: CGSize(width: 1000, height: 1500), in: CGSize(width: 1000, height: 380), focus: CGPoint(x: 0.5, y: 0))
        XCTAssertEqual(top.origin.y, 0)
        let bottom = HeroFraming.place(image: CGSize(width: 1000, height: 1500), in: CGSize(width: 1000, height: 380), focus: CGPoint(x: 0.5, y: 1))
        XCTAssertEqual(bottom.origin.y, 380 - 1500, accuracy: 1e-9)
    }
    /// 横拍合照：横向把脸挪到中间
    func testWidePhotoCentersFacesHorizontally() {
        let placed = HeroFraming.place(image: CGSize(width: 3000, height: 1000), in: CGSize(width: 900, height: 380), focus: CGPoint(x: 0.8, y: 0.4))
        XCTAssertEqual(placed.size.height, 380, accuracy: 1e-9)
        XCTAssertEqual(placed.origin.x, max(900 - 1140, 0.5 * 900 - 0.8 * 1140), accuracy: 1e-9)
    }
    func testFocusUnionsFaces() {
        let point = HeroFraming.focus(faces: [CGRect(x: 0.1, y: 0.5, width: 0.1, height: 0.1), CGRect(x: 0.7, y: 0.5, width: 0.1, height: 0.1)])
        XCTAssertEqual(point!.x, 0.45, accuracy: 1e-9)
        XCTAssertEqual(point!.y, 0.55 - 0.015, accuracy: 1e-9)
        XCTAssertNil(HeroFraming.focus(faces: []))
    }
}
