import XCTest
@testable import A2UIRuntime

final class AdapterTests: XCTestCase {
    private func loadLines(name: String) throws -> [String] {
        let url = Bundle.module.url(forResource: name, withExtension: "jsonl")
            ?? Bundle.module.url(forResource: name, withExtension: "jsonl", subdirectory: "Fixtures")
        XCTAssertNotNil(url, "Missing fixture \(name).jsonl")
        let data = try Data(contentsOf: url!)
        let content = String(decoding: data, as: UTF8.self)
        return content.split(separator: "\n").map { String($0) }
    }

    @MainActor
    func testV08FixtureAppliesAndResolvesBindings() async throws {
        let adapter = A2UIProtocolAdapter(enableV09: true)
        let store = NormalizedSurfaceStore()
        for line in try loadLines(name: "v0.8") {
            store.apply(adapter.decode(line: line))
        }

        XCTAssertEqual(store.rootComponentId(for: "s1"), "root")
        let resolved = store.resolvedProps(surfaceId: "s1", componentId: "nameField")
        XCTAssertEqual(resolved?["value"], .string("Ada"))
        XCTAssertNil(store.lastError)
    }

    @MainActor
    func testV09FixtureAppliesAndResolvesBindings() async throws {
        let adapter = A2UIProtocolAdapter(enableV09: true)
        let store = NormalizedSurfaceStore()
        for line in try loadLines(name: "v0.9") {
            store.apply(adapter.decode(line: line))
        }

        XCTAssertEqual(store.rootComponentId(for: "s2"), "root")
        let resolved = store.resolvedProps(surfaceId: "s2", componentId: "emailField")
        XCTAssertEqual(resolved?["value"], .string("ada@example.com"))
        XCTAssertNil(store.lastError)
    }

    func testUnknownMessageReturnsError() {
        let adapter = A2UIProtocolAdapter(enableV09: true)
        let messages = adapter.decode(line: "{\"unknown\":{\"foo\":1}}")
        XCTAssertEqual(messages.count, 1)
        if case let .error(text) = messages[0] {
            XCTAssertTrue(text.contains("unsupported"))
        } else {
            XCTFail("Expected error message")
        }
    }

    func testJSONPointerUpdate() {
        var model: JSONValue = .object([:])
        model.setValue(.string("value"), at: "/a/b/0")
        XCTAssertEqual(model.value(at: "/a/b/0"), .string("value"))
    }
}
