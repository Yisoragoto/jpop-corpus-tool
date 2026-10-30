import Foundation

/// 库里的音频、封面、歌手照片在这台电脑上找不找得到（Rust `mediaStatus`，和播放、显示用同一套找法）。
/// 只数「记了路径却找不到」的，从别的电脑拷来的库会有很多
public struct MediaStatus: Codable, Hashable, Sendable {
    public struct Folder: Codable, Hashable, Sendable {
        public var folder: String
        public var songs: Int
    }

    public var songs: Int
    public var audioMissing: Int
    public var coversMissing: Int
    public var albumsMissing: Int
    public var artistsMissing: Int
    /// 找不到的音频原来在哪些文件夹，歌多的在前
    public var missingAudioFolders: [Folder]

    public var anythingMissing: Bool { audioMissing + coversMissing + albumsMissing + artistsMissing > 0 }
}

extension CorpusClient {
    /// 要逐个找文件（找不到时会往下翻几层目录），放在后台队列，不占会话连接
    public func mediaStatus() async throws -> MediaStatus {
        try await performDetached { try $0.call("mediaStatus") }
    }
}
