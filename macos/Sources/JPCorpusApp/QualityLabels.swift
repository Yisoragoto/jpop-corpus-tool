import Foundation
import JPCorpusModel

// 来源追踪、语料质量里的代码 → 中文。和 Tauri 前端 app/src/corpusQuality.ts 同一套说法

let issueLabels: [String: String] = [
    "empty": "一行歌词都没有",
    "replacement_char": "有乱码",
    "not_japanese": "看起来不是日文歌词",
    "too_few_lines": "行数太少",
    "ends_early": "提前结束，可能被截断",
    "untimed": "没有时间轴",
    "mixed_timing": "部分行没有时间",
    "non_monotonic": "时间倒退",
    "beyond_duration": "时间超过歌曲长度",
    "duplicate_timestamp": "和上一行同一个时间",
    "stacked_duplicate": "重复贴了一遍",
    "long_gap": "长空档（间奏或漏行）",
    "leftover_markup": "混进了时间标记或署名",
    "very_long_line": "一行特别长",
]

let languageLabels: [String: String] = [
    "ja": "日文", "en": "英文", "ko": "韩文", "zh": "中文", "mixed": "混合", "unknown": "未知", "": "未判断",
]

let lyricSourceLabels: [String: String] = [
    "lrc_file": "导入时的 .lrc",
    "embedded": "音频内嵌",
    "sibling": "音频旁的 .lrc",
    "online": "在线",
    "manual_file": "你选的文件",
    "restored": "从旧版恢复",
    "manual_edit": "逐行编辑",
    "legacy": "老库（未记来源）",
]

let fieldSourceLabels: [String: String] = [
    "tag": "文件标签", "filename": "文件名", "folder": "所在目录", "manual": "你改的", "heuristic": "按歌词推断",
    "audio": "音频文件", "itunes": "iTunes", "musicbrainz": "MusicBrainz", "deezer": "Deezer",
]

let fieldLabels: [String: String] = [
    "language": "语言", "title": "曲名", "artist": "歌手", "album": "专辑", "year": "年份", "genre": "流派",
    "cover": "封面", "duration": "时长", "release_date": "发行日期",
]

let tokenStatusLabels: [String: String] = [
    "auto": "自动分词", "corrected": "校正过", "pending": "未分词", "empty": "无实词", "unknown": "未记录",
]

func codeLabel(_ table: [String: String], _ code: String) -> String {
    table[code] ?? code
}
