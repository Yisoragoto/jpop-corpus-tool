import Foundation
public enum AvatarIdentity {
    public static func monogram(_ name: String) -> String {
        let value = name.trimmingCharacters(in: .whitespacesAndNewlines)
        if value.isEmpty { return "♪" }
        if let r = value.range(of: "^[0-9]{1,3}", options: .regularExpression) { return String(value[r]) }
        let words = value.components(separatedBy: CharacterSet.whitespacesAndNewlines.union(CharacterSet(charactersIn: "・"))).filter { !$0.isEmpty }
        if words.count > 1 && words.allSatisfy({ $0.range(of: "^[a-z]", options: [.regularExpression, .caseInsensitive]) != nil }) {
            return words.prefix(2).map { String(String.UnicodeScalarView($0.unicodeScalars.prefix(1))) }.joined().uppercased()
        }
        return String(String.UnicodeScalarView(value.unicodeScalars.prefix(2))).uppercased()
    }
    public static func avatarTone(_ name: String) -> Int {
        var hash: UInt32 = 0
        for ch in name.unicodeScalars { hash = hash &* 31 &+ ch.value }
        return Int(hash % 4)
    }
}
