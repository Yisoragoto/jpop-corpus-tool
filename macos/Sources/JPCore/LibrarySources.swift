import Foundation

// 来源文件夹（#60）：曲库从哪些文件夹读歌。和 Windows 的 LibrarySources 同一套 Rust 命令
// （jp-api 的 library_source_* 和 jp-ffi 补上的建议 / 移除 / 歌词同步）。扫描只对账，确认了才写库。

/// 一个音乐文件夹（Rust 的 `Source`）
public struct LibrarySourceInfo: Codable, Hashable, Sendable {
    public var id: Int64
    public var rootPath: String
    public var label: String
    public var recursive: Bool
    public var enabled: Bool
    public var createdAt: String
    public var lastScannedAt: String?
    public var lastScanError: String?
}

/// 列表上的一行（Rust 的 `SourceView`：来源本身摊平，再加歌数和文件夹在不在）
public struct LibrarySource: Codable, Hashable, Identifiable, Sendable {
    public var id: Int64
    public var rootPath: String
    public var label: String
    public var recursive: Bool
    public var enabled: Bool
    public var createdAt: String
    public var lastScannedAt: String?
    public var lastScanError: String?
    /// 在库的歌
    public var songs: Int
    /// 已移出的歌
    public var removed: Int
    /// 文件夹现在在不在（盘没插、挪走了是 false）
    public var present: Bool
}

public struct SourceFolderInput: Codable, Hashable, Sendable {
    public var path: String
    public var recursive: Bool
    public init(path: String, recursive: Bool = true) { self.path = path; self.recursive = recursive }
}

/// 一首歌和它的文件。`songId` 在「文件变了」里可能是空串（认不出是哪首）
public struct SourceSongFile: Codable, Hashable, Sendable {
    public var songId: String
    public var title: String
    public var artist: String
    public var path: String
}

public struct SourceMovedSong: Codable, Hashable, Sendable {
    public var songId: String
    public var title: String
    public var artist: String
    public var from: String
    public var to: String
    /// 凭什么认定是同一首
    public var reason: String
    public var wasRemoved: Bool
}

/// 一个文件夹的对账（Rust 的 `SourcePlanView`）
public struct SourcePlan: Codable, Hashable, Sendable {
    public var source: LibrarySourceInfo
    /// 库里没有的文件。不在这里导入：拿去导入页复核
    public var newFiles: [String]
    public var newSongs: Int
    public var changed: [SourceSongFile]
    public var returned: [SourceSongFile]
    public var moved: [SourceMovedSong]
    public var gone: [SourceSongFile]
    /// 音频没变、旁边的歌词文件新放进来或改过（`path` 是歌词文件）
    public var lyricsChanged: [SourceSongFile]
    public var unchanged: Int
    /// 一个音频都没扫到（可能是盘换了内容）
    public var nothingFound: Bool
}

public struct SourceRelink: Codable, Hashable, Sendable {
    public var songId: String
    public var to: String
    public init(songId: String, to: String) { self.songId = songId; self.to = to }
}

/// 确认写哪几组。没勾的组传空
public struct SourceApplyRequest: Codable, Hashable, Sendable {
    public var refresh: [String] = []
    public var restore: [String] = []
    public var relink: [SourceRelink] = []
    public var moveOut: [String] = []
    public init(refresh: [String] = [], restore: [String] = [], relink: [SourceRelink] = [], moveOut: [String] = []) {
        self.refresh = refresh; self.restore = restore; self.relink = relink; self.moveOut = moveOut
    }
}

public struct SourceApplied: Codable, Hashable, Sendable {
    public var refreshed: Int
    public var restored: Int
    public var relinked: Int
    public var movedOut: Int
    /// 对账到确认之间情况变了、没有写的几项
    public var skipped: Int
}

/// 已移出的歌
public struct RemovedSong: Codable, Hashable, Identifiable, Sendable {
    public var id: String
    public var title: String
    public var artist: String
    public var audioPath: String
    public var removedAt: String
    /// 原来属于哪个文件夹（那个文件夹也移除了是空串）
    public var source: String
    public var audioPresent: Bool
}

public struct SourceSuggestion: Codable, Hashable, Sendable {
    public var path: String
    public var recursive: Bool
    public var songs: Int
}

/// 还没有来源时，按库里歌的位置建议的文件夹
public struct SourceSuggestions: Codable, Hashable, Sendable {
    public var folders: [SourceSuggestion]
    /// 全加上之后还有几首不在任何文件夹里
    public var looseSongs: Int
}

/// 移除文件夹时它的歌怎么办
public enum SourceRemoveMode: String, Codable, Sendable {
    /// 移出语料库，可以恢复
    case moveOut
    /// 彻底删除
    case delete
}

public struct SourceRemoved: Codable, Hashable, Sendable {
    public var movedOut: Int
    public var deleted: Int
    public var sources: [LibrarySource]
}

public struct LyricSyncFailure: Codable, Hashable, Sendable {
    public var songId: String
    public var error: String
}

public struct LyricSyncReport: Codable, Hashable, Sendable {
    public var updated: [String]
    public var unchanged: Int
    public var failed: [LyricSyncFailure]
}

extension CorpusClient {
    public func librarySources() async throws -> [LibrarySource] {
        try await perform { try $0.call("librarySources") }
    }
    public func librarySourcesAdd(_ folders: [SourceFolderInput]) async throws -> [LibrarySource] {
        try await perform { try $0.call("librarySourcesAdd", ["folders": AnyEncodable(folders)]) }
    }
    public func librarySourceSetEnabled(_ id: Int64, enabled: Bool) async throws -> [LibrarySource] {
        try await perform { try $0.call("librarySourceSetEnabled", ["id": AnyEncodable(id), "enabled": AnyEncodable(enabled)]) }
    }
    /// 扫一遍文件夹对账，只读。要走完整个文件夹，期间同一队列上的别的调用要等
    public func librarySourcePlan(_ id: Int64) async throws -> SourcePlan {
        try await perform { try $0.call("librarySourcePlan", ["id": AnyEncodable(id)]) }
    }
    public func librarySourceApply(_ id: Int64, _ request: SourceApplyRequest) async throws -> SourceApplied {
        try await perform {
            try $0.call("librarySourceApply", [
                "id": AnyEncodable(id), "refresh": AnyEncodable(request.refresh), "restore": AnyEncodable(request.restore),
                "relink": AnyEncodable(request.relink), "moveOut": AnyEncodable(request.moveOut),
            ])
        }
    }
    public func libraryRemovedSongs() async throws -> [RemovedSong] {
        try await perform { try $0.call("libraryRemovedSongs") }
    }
    public func libraryRestoreSongs(_ ids: [String]) async throws -> Int {
        try await perform { try $0.call("libraryRestoreSongs", ["ids": AnyEncodable(ids)]) }
    }
    public func librarySourceSuggestions() async throws -> SourceSuggestions {
        try await performDetached { try $0.call("librarySourceSuggestions") }
    }
    public func librarySourceRemove(_ id: Int64, mode: SourceRemoveMode) async throws -> SourceRemoved {
        try await performDetached { try $0.call("librarySourceRemove", ["id": AnyEncodable(id), "mode": AnyEncodable(mode)]) }
    }
    /// 要分词，慢；用独立连接，不占会话队列
    public func librarySyncLyrics(_ songIds: [String]) async throws -> LyricSyncReport {
        try await performDetached { try $0.call("librarySyncLyrics", ["songIds": AnyEncodable(songIds)]) }
    }
}
