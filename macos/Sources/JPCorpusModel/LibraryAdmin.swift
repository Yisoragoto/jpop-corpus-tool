import JPCore
public enum LibraryAdmin {
    public static let audioExtensions = ["flac", "mp3", "wav", "m4a", "ogg", "aac"]
    public static let fieldLabels = ["title": "歌名", "artist": "歌手", "year": "年份", "album": "专辑", "genre": "流派"]
    public static func csvProblem(_ csv: String?) -> String? {
        guard let csv, !["updated", "noFile", "unchanged"].contains(csv) else { return nil }; return csv
    }
    public static func describeEdit(_ r: EditReport) -> String {
        if r.changed.isEmpty { return "没有改动" }
        var parts = ["改了" + r.changed.map { fieldLabels[$0] ?? $0 }.joined(separator: "、")]
        if r.performers > 0 { parts.append("演唱署名重写为 \(r.performers) 人") }
        if r.correctionsRenamed > 0 { parts.append("\(r.correctionsRenamed) 条分词校正跟着改了歌名歌手") }
        if r.csv == "updated" { parts.append("songs.csv 已同步") }
        return parts.joined(separator: "，")
    }
    public static func describeDelete(_ r: DeleteResult) -> String {
        var parts = ["已删除《\(r.title)》：\(r.lyricLines) 行歌词、\(r.tokens) 个词"]
        if r.plays > 0 { parts.append("\(r.plays) 条收听记录") }
        if r.albumsRemoved > 0 { parts.append("\(r.albumsRemoved) 张空专辑") }
        if r.peopleRemoved > 0 { parts.append("\(r.peopleRemoved) 个没有作品了的人") }
        if r.coverRemoved { parts.append("下载的封面") }
        let kept = r.correctionsKept > 0 ? "；\(r.correctionsKept) 条分词校正留着，重新导入时恢复" : ""
        return parts.joined(separator: "、") + kept + "。音频文件没有动。"
    }
}
