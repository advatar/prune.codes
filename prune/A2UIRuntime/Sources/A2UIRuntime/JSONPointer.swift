import Foundation

enum JSONPointer {
    static func tokens(from pointer: String) -> [String] {
        guard !pointer.isEmpty else { return [] }
        let trimmed = pointer.hasPrefix("/") ? String(pointer.dropFirst()) : pointer
        if trimmed.isEmpty { return [] }
        return trimmed.split(separator: "/").map { token in
            token
                .replacingOccurrences(of: "~1", with: "/")
                .replacingOccurrences(of: "~0", with: "~")
        }
    }
}
