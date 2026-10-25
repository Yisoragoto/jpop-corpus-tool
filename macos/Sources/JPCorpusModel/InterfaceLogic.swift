import Foundation
import JPCore

public enum InterfaceLogic {
    public static func formatRelative(_ iso: String, now: Date = Date()) -> String {
        let parser = ISO8601DateFormatter()
        parser.formatOptions = [.withInternetDateTime,.withFractionalSeconds]
        let parsed = parser.date(from: iso)
        parser.formatOptions = [.withInternetDateTime]
        guard let then = parsed ?? parser.date(from: iso) else { return "" }
        let s = max(0,now.timeIntervalSince(then))
        if s < 60 { return "刚刚" }; if s < 3600 { return "\(Int(s/60)) 分钟前" }
        if s < 86400 { return "\(Int(s/3600)) 小时前" }; if s < 86400*60 { return "\(Int(s/86400)) 天前" }
        return "\(Int(s/(86400*30))) 个月前"
    }
    public static func isPlayingSpan(currentSong: String?, playing: Bool, position: Double, songId: String, startSec: Double?, endSec: Double?) -> Bool {
        guard currentSong == songId, playing, let start = startSec else { return false }
        return position >= start-0.5 && position < (endSec ?? start+5)
    }
    public static func backfillNote(songs: Int,tokens: Int,error: String) -> String {
        if !error.isEmpty { return "顺手补分词没成功（库没被改动）：\(error)。可以到「曲库维护 → 补齐缺失分词」再试。" }
        if songs == 0 { return "" }
        let formatter = NumberFormatter(); formatter.numberStyle = .decimal; formatter.locale = Locale(identifier: "zh_CN")
        return "顺手给 \(songs) 首没分词的歌补上了分词（\(formatter.string(from: NSNumber(value: tokens)) ?? String(tokens)) 个词），那几首现在能查词、有振假名了。"
    }
}
