import CoreGraphics

/// 音乐人头图怎么裁：照片按比例铺满横幅，再挪到让「焦点」（人脸，找不到脸就是显著区域）落在合适的位置。
/// 以前固定靠上裁一个正方形，横拍的合照、人站在画面下半部的照片经常只剩背景
public enum HeroFraming {
    /// 焦点在横幅里放在哪：横向居中，纵向偏上（下半截压暗、写名字）
    public static let target = CGPoint(x: 0.5, y: 0.4)

    /// 返回照片铺满后的尺寸和左上角的偏移。`focus` 是照片里的归一化坐标（左上为原点）
    public static func place(image: CGSize, in frame: CGSize, focus: CGPoint) -> (size: CGSize, origin: CGPoint) {
        guard image.width > 0, image.height > 0, frame.width > 0, frame.height > 0 else { return (frame, .zero) }
        let scale = max(frame.width / image.width, frame.height / image.height)
        let size = CGSize(width: image.width * scale, height: image.height * scale)
        func axis(_ want: CGFloat, _ focus: CGFloat, _ drawn: CGFloat, _ room: CGFloat) -> CGFloat {
            // 偏移只能在 [room - drawn, 0] 之间，不然会露出空白
            min(0, max(room - drawn, want * room - focus * drawn))
        }
        let origin = CGPoint(x: axis(target.x, focus.x, size.width, frame.width),
                             y: axis(target.y, focus.y, size.height, frame.height))
        return (size, origin)
    }

    /// 多张脸取并起来的那块区域的中心；脸靠近上沿时往下挪一点，留出头顶（Vision 的框只到额头）
    public static func focus(faces: [CGRect]) -> CGPoint? {
        guard var union = faces.first else { return nil }
        for face in faces.dropFirst() { union = union.union(face) }
        return CGPoint(x: union.midX, y: max(0, union.midY - union.height * 0.15))
    }
}
