import CMarkit
import Foundation

public struct MarkitDocument: Decodable, Sendable {
    public let version: Int
    public let text: String
    public let spans: [MarkitSpan]
}

public struct MarkitSpan: Decodable, Sendable {
    public let start: Int
    public let end: Int
    public let kind: String
    public let level: Int?
    public let depth: Int?
    public let language: String?
    public let url: String?

    public var range: NSRange { NSRange(location: start, length: end - start) }
}

public enum MarkitError: Error {
    case renderFailed(status: UInt32)
}

public enum Markit {
    /// Synchronous, thread-safe parsing. Call off the main thread for large documents.
    /// The application must link libmarkit_ffi.a for its destination architecture.
    public static func parse(_ source: String) throws -> MarkitDocument {
        let bytes = Array(source.utf8)
        let buffer = bytes.withUnsafeBufferPointer {
            markit_render($0.baseAddress, $0.count, 0)
        }
        defer { markit_buffer_free(buffer) }
        guard buffer.status == 0, let pointer = buffer.data else {
            throw MarkitError.renderFailed(status: buffer.status)
        }
        let data = Data(bytes: pointer, count: buffer.len)
        return try JSONDecoder().decode(MarkitDocument.self, from: data)
    }
}
