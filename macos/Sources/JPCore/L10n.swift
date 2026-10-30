import Foundation

/// 英文界面。和 Windows 的 `tr` / `trBackend` 一个思路：键是中文原文，显示前整句换成英文。
///
/// 表在 .app 的 Resources/l10n-en.json（scripts/gen-l10n.py 生成：Windows 的翻译 + macOS 自己的）。
/// 没有插值的整句直接查；带插值的（「补齐 {0} 张专辑封面」、Rust 的报错）按模板匹配，
/// 填进去的值本身是中文的话再翻一次（「打不开语料库：{0}」里的 {0} 也是一句后端的话）。
///
/// 歌词、歌名这些数据也会经过这里（`Text(String)` 统一走它），所以：
/// 带假名的只做整句精确匹配，不套模板（日文歌词不会被拆开翻）；结果都缓存，卡拉 OK 每秒 30 帧也不重算
public final class L10n: @unchecked Sendable {
    public static let languageKey = "ui.language"
    public static let shared = L10n()

    /// 当前是不是英文界面
    public static var isEnglish: Bool { shared.english }
    public static func t(_ text: String) -> String { shared.translate(text) }

    /// 用户选的语言（"zh" / "en"），没选过时跟随系统：系统首选是中文就中文，否则英文
    public static func preferredLanguage(_ defaults: UserDefaults = .standard) -> String {
        if let chosen = defaults.string(forKey: languageKey), chosen == "zh" || chosen == "en" { return chosen }
        return (Locale.preferredLanguages.first ?? "zh").hasPrefix("zh") ? "zh" : "en"
    }

    /// 换语言：存下来，并同步 AppleLanguages（系统自己的那部分——菜单、提示框标题——下次启动跟着换）。
    /// nil = 跟随系统
    public static func choose(_ language: String?, defaults: UserDefaults = .standard) {
        if let language {
            defaults.set(language, forKey: languageKey)
            defaults.set([language == "en" ? "en" : "zh-Hans"], forKey: "AppleLanguages")
        } else {
            defaults.removeObject(forKey: languageKey)
            defaults.removeObject(forKey: "AppleLanguages")
        }
        shared.reload(english: preferredLanguage(defaults) == "en")
    }

    private struct Pattern {
        let anchor: String
        let regex: NSRegularExpression
        let english: String
    }

    private let lock = NSRecursiveLock()
    private var english: Bool
    private let tableURL: URL?
    private var loaded = false
    private var exact: [String: String] = [:]
    private var patterns: [Pattern] = []
    private var cache: [String: String] = [:]

    init(english: Bool? = nil, table: URL? = Bundle.main.url(forResource: "l10n-en", withExtension: "json")) {
        self.english = english ?? (L10n.preferredLanguage() == "en")
        tableURL = table
    }

    /// 测试用：直接给表
    public convenience init(english: Bool, json: Data) {
        self.init(english: english, table: nil)
        load(json)
    }

    func reload(english: Bool) {
        lock.lock(); defer { lock.unlock() }
        self.english = english
        cache.removeAll()
    }

    public func translate(_ text: String) -> String {
        guard english, text.unicodeScalars.contains(where: L10n.isHan) else { return text }
        lock.lock(); defer { lock.unlock() }
        if !loaded, let tableURL, let data = try? Data(contentsOf: tableURL) { load(data) }
        loaded = true
        if let hit = cache[text] { return hit }
        let result = lookup(text, depth: 0)
        if cache.count > 20_000 { cache.removeAll() }
        cache[text] = result
        return result
    }

    private func lookup(_ text: String, depth: Int) -> String {
        if let hit = exact[text] { return hit }
        // 两头的空白不算：「 首」这类拼出来的片段
        let trimmed = text.trimmingCharacters(in: .whitespaces)
        if trimmed != text, let hit = exact[trimmed] { return text.replacingOccurrences(of: trimmed, with: hit) }
        // 带假名的是日文（歌词、歌名）：不套模板。除非里面有日文不用的简体字（「作词：ACAね」这种署名）
        guard depth < 3, !text.unicodeScalars.contains(where: L10n.isKana) || text.contains(where: { L10n.chineseOnly.contains($0) })
        else { return text }
        let range = NSRange(text.startIndex..., in: text)
        for pattern in patterns where text.contains(pattern.anchor) {
            guard let match = pattern.regex.firstMatch(in: text, range: range) else { continue }
            var out = pattern.english
            for index in (0..<(match.numberOfRanges - 1)).reversed() {
                guard let r = Range(match.range(at: index + 1), in: text) else { continue }
                let value = String(text[r])
                let translated = value.unicodeScalars.contains(where: L10n.isHan) ? lookup(value, depth: depth + 1) : value
                out = out.replacingOccurrences(of: "{\(index)}", with: translated)
            }
            return out
        }
        // 几行拼成的一段：逐行翻
        if text.contains("\n") {
            let lines = text.components(separatedBy: "\n")
            let done = lines.map { lookup($0, depth: depth + 1) }
            if done != lines { return done.joined(separator: "\n") }
        }
        return text
    }

    private func load(_ data: Data) {
        loaded = true
        guard let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { return }
        exact = object["exact"] as? [String: String] ?? [:]
        var built: [(score: Int, pattern: Pattern)] = []
        for pair in object["patterns"] as? [[String]] ?? [] where pair.count == 2 {
            let key = pair[0]
            // 键按 {n} 切开：字面的部分转义，占位符换成捕获组。模板里占位符可能不按顺序出现在英文里，
            // 但在中文键里一定是 {0} {1} … 依次出现（生成脚本保证）
            var regex = "^", literal = "", longest = "", rest = Substring(key)
            while let open = rest.range(of: "{") {
                let close = rest[open.upperBound...].firstIndex(of: "}")
                guard let close, Int(rest[open.upperBound..<close]) != nil else {
                    literal += rest[..<open.upperBound]; rest = rest[open.upperBound...]; continue
                }
                literal += rest[..<open.lowerBound]
                regex += NSRegularExpression.escapedPattern(for: literal) + "(.+?)"
                if literal.count > longest.count { longest = literal }
                literal = ""
                rest = rest[rest.index(after: close)...]
            }
            literal += rest
            regex += NSRegularExpression.escapedPattern(for: literal) + "$"
            if literal.count > longest.count { longest = literal }
            guard !longest.isEmpty, let compiled = try? NSRegularExpression(pattern: regex, options: [.dotMatchesLineSeparators]) else { continue }
            let score = key.replacingOccurrences(of: #"\{\d+\}"#, with: "", options: .regularExpression).count
            built.append((score, Pattern(anchor: longest, regex: compiled, english: pair[1])))
        }
        // 字面部分越长越具体，先试：「打不开语料库：{0}」要排在「{0}：{1}」前面
        patterns = built.sorted { $0.score > $1.score }.map { $0.pattern }
    }

    /// 只在简体中文里出现、日文不用的字（日文写作 詞 説 這 時 対 設 開 関 …）
    static let chineseOnly = Set("们这说个么为从词选择发应该时对设导认开关显单纯样查读库听专辑张还让给过请无错误换载据删创编隐")
    static func isHan(_ s: Unicode.Scalar) -> Bool { (0x4E00...0x9FFF).contains(s.value) || (0x3400...0x4DBF).contains(s.value) }
    static func isKana(_ s: Unicode.Scalar) -> Bool { (0x3040...0x30FF).contains(s.value) }
}
