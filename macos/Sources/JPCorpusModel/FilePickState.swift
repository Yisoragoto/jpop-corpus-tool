/// Presentation dismissal can arrive before the completion callback. Keep the intent until finish().
public struct FilePickState<Intent> {
    public private(set) var intent: Intent?
    public var isPresented = false

    public init() {}

    public mutating func present(_ intent: Intent) {
        self.intent = intent
        isPresented = true
    }

    public mutating func finish() -> Intent? {
        defer {
            intent = nil
            isPresented = false
        }
        return intent
    }
}

public enum ImportPickKind: Equatable {
    case folders, files, dictionary
}
