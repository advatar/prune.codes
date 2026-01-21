import Foundation
import Combine

// MARK: - Protocol versions

public enum A2UIProtocolVersion: String, CaseIterable, Codable {
    case v08 = "0.8"
    case v09 = "0.9"
}

// MARK: - JSONValue

/// A lightweight JSON value type used throughout the A2UI runtime.
///
/// We keep this type small and dependency-free so it can be used both for
/// protocol decoding and for data-model storage.
public enum JSONValue: Hashable, Codable {
    case null
    case bool(Bool)
    case number(Double)
    case string(String)
    case array([JSONValue])
    case object([String: JSONValue])

    public init(from decoder: Decoder) throws {
        let c = try decoder.singleValueContainer()
        if c.decodeNil() {
            self = .null
            return
        }
        if let b = try? c.decode(Bool.self) {
            self = .bool(b)
            return
        }
        if let n = try? c.decode(Double.self) {
            self = .number(n)
            return
        }
        if let s = try? c.decode(String.self) {
            self = .string(s)
            return
        }
        if let a = try? c.decode([JSONValue].self) {
            self = .array(a)
            return
        }
        if let o = try? c.decode([String: JSONValue].self) {
            self = .object(o)
            return
        }
        throw DecodingError.dataCorruptedError(in: c, debugDescription: "Unsupported JSON value")
    }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.singleValueContainer()
        switch self {
        case .null:
            try c.encodeNil()
        case .bool(let b):
            try c.encode(b)
        case .number(let n):
            try c.encode(n)
        case .string(let s):
            try c.encode(s)
        case .array(let a):
            try c.encode(a)
        case .object(let o):
            try c.encode(o)
        }
    }

    public var stringValue: String? {
        if case .string(let s) = self { return s }
        return nil
    }

    public var boolValue: Bool? {
        if case .bool(let b) = self { return b }
        return nil
    }

    public var numberValue: Double? {
        if case .number(let n) = self { return n }
        return nil
    }

    public var arrayValue: [JSONValue]? {
        if case .array(let a) = self { return a }
        return nil
    }

    public var objectValue: [String: JSONValue]? {
        if case .object(let o) = self { return o }
        return nil
    }

    public static func fromAny(_ any: Any?) -> JSONValue {
        guard let any else { return .null }
        switch any {
        case is NSNull:
            return .null
        case let b as Bool:
            return .bool(b)
        case let i as Int:
            return .number(Double(i))
        case let d as Double:
            return .number(d)
        case let f as Float:
            return .number(Double(f))
        case let s as String:
            return .string(s)
        case let a as [Any]:
            return .array(a.map { JSONValue.fromAny($0) })
        case let o as [String: Any]:
            var out: [String: JSONValue] = [:]
            for (k, v) in o {
                out[k] = JSONValue.fromAny(v)
            }
            return .object(out)
        default:
            // Fallback: try to stringify.
            return .string(String(describing: any))
        }
    }

    public func toAny() -> Any {
        switch self {
        case .null:
            return NSNull()
        case .bool(let b):
            return b
        case .number(let n):
            return n
        case .string(let s):
            return s
        case .array(let a):
            return a.map { $0.toAny() }
        case .object(let o):
            var out: [String: Any] = [:]
            for (k, v) in o {
                out[k] = v.toAny()
            }
            return out
        }
    }
}

// MARK: - Normalized messages

public enum NormalizedMsg: Hashable {
    case createSurface(surfaceId: String, catalogId: String?, rootComponentId: String?)
    case deleteSurface(surfaceId: String)
    case updateComponents(surfaceId: String, components: [NormalizedComponent])
    case updateDataModel(surfaceId: String, updates: [DataModelUpdate])
    case error(String)

    public struct DataModelUpdate: Hashable {
        public var path: String
        public var value: JSONValue
        public init(path: String, value: JSONValue) {
            self.path = path
            self.value = value
        }
    }
}

public struct NormalizedComponent: Identifiable, Hashable {
    public var id: String
    public var type: String
    public var props: [String: JSONValue]

    public init(id: String, type: String, props: [String: JSONValue] = [:]) {
        self.id = id
        self.type = type
        self.props = props
    }
}

public struct SurfaceState: Hashable {
    public var surfaceId: String
    public var catalogId: String?
    public var rootComponentId: String?
    public var components: [String: NormalizedComponent]
    public var dataModel: JSONValue

    public init(surfaceId: String, catalogId: String?, rootComponentId: String?) {
        self.surfaceId = surfaceId
        self.catalogId = catalogId
        self.rootComponentId = rootComponentId
        self.components = [:]
        self.dataModel = .object([:])
    }
}

// MARK: - Store

@MainActor
public final class NormalizedSurfaceStore: ObservableObject {
    @Published public private(set) var surfaces: [String: SurfaceState] = [:]

    public init() {}

    /// Drops all known surfaces and state.
    public func reset() {
        surfaces = [:]
    }

    public func apply(_ msg: NormalizedMsg) {
        switch msg {
        case .createSurface(let surfaceId, let catalogId, let rootComponentId):
            var s = SurfaceState(surfaceId: surfaceId, catalogId: catalogId, rootComponentId: rootComponentId)
            // Preserve old state if surface already existed.
            if let old = surfaces[surfaceId] {
                s.components = old.components
                s.dataModel = old.dataModel
            }
            surfaces[surfaceId] = s

        case .deleteSurface(let surfaceId):
            surfaces.removeValue(forKey: surfaceId)

        case .updateComponents(let surfaceId, let components):
            guard var s = surfaces[surfaceId] else {
                // If components arrive before createSurface, create implicitly.
                surfaces[surfaceId] = SurfaceState(surfaceId: surfaceId, catalogId: nil, rootComponentId: nil)
                return apply(.updateComponents(surfaceId: surfaceId, components: components))
            }
            for c in components {
                s.components[c.id] = c
            }
            surfaces[surfaceId] = s

        case .updateDataModel(let surfaceId, let updates):
            guard var s = surfaces[surfaceId] else {
                surfaces[surfaceId] = SurfaceState(surfaceId: surfaceId, catalogId: nil, rootComponentId: nil)
                return apply(.updateDataModel(surfaceId: surfaceId, updates: updates))
            }
            for u in updates {
                s.dataModel = JSONPointer.set(s.dataModel, path: u.path, value: u.value)
            }
            surfaces[surfaceId] = s

        case .error:
            // No-op in the store.
            break
        }
    }

    public func rootComponentId(for surfaceId: String) -> String? {
        surfaces[surfaceId]?.rootComponentId
    }

    public func component(surfaceId: String, componentId: String) -> NormalizedComponent? {
        surfaces[surfaceId]?.components[componentId]
    }

    public func dataModelValue(surfaceId: String, path: String) -> JSONValue? {
        guard let s = surfaces[surfaceId] else { return nil }
        return JSONPointer.get(s.dataModel, path: path)
    }

    public func setDataModelValue(surfaceId: String, path: String, value: JSONValue) {
        apply(.updateDataModel(surfaceId: surfaceId, updates: [.init(path: path, value: value)]))
    }

    /// Returns a props map where `{ "path": "/foo" }` bindings are resolved using the current data model.
    public func resolvedProps(surfaceId: String, componentId: String) -> [String: JSONValue] {
        guard let s = surfaces[surfaceId], let c = s.components[componentId] else { return [:] }
        var out: [String: JSONValue] = [:]
        for (k, v) in c.props {
            if let p = JSONPointer.bindingPath(from: v), let resolved = JSONPointer.get(s.dataModel, path: p) {
                out[k] = resolved
            } else {
                out[k] = v
            }
        }
        return out
    }

    /// Direct, raw props as stored in the surface (unresolved).
    public func rawProps(surfaceId: String, componentId: String) -> [String: JSONValue] {
        guard let s = surfaces[surfaceId], let c = s.components[componentId] else { return [:] }
        return c.props
    }
}

// MARK: - JSON Pointer helpers

enum JSONPointer {
    static func bindingPath(from v: JSONValue) -> String? {
        guard case .object(let o) = v else { return nil }
        if let p = o["path"], case .string(let s) = p {
            return s
        }
        return nil
    }

    static func decodeTokens(_ path: String) -> [String] {
        // JSON pointer: /a/b/~1c
        let raw = path.split(separator: "/", omittingEmptySubsequences: true)
        return raw.map {
            $0
                .replacingOccurrences(of: "~1", with: "/")
                .replacingOccurrences(of: "~0", with: "~")
        }
    }

    static func get(_ root: JSONValue, path: String) -> JSONValue? {
        let toks = decodeTokens(path)
        if toks.isEmpty { return root }
        var cur = root
        for t in toks {
            switch cur {
            case .object(let o):
                guard let nxt = o[t] else { return nil }
                cur = nxt
            case .array(let a):
                guard let idx = Int(t), idx >= 0, idx < a.count else { return nil }
                cur = a[idx]
            default:
                return nil
            }
        }
        return cur
    }

    static func set(_ root: JSONValue, path: String, value: JSONValue) -> JSONValue {
        let toks = decodeTokens(path)
        guard !toks.isEmpty else { return value }
        return setRec(root, toks: toks[...], value: value)
    }

    private static func setRec(_ cur: JSONValue, toks: ArraySlice<String>, value: JSONValue) -> JSONValue {
        guard let head = toks.first else { return value }
        let tail = toks.dropFirst()

        if tail.isEmpty {
            // Set at leaf.
            switch cur {
            case .object(var o):
                o[head] = value
                return .object(o)
            case .array(var a):
                if let idx = Int(head), idx >= 0 {
                    if idx < a.count {
                        a[idx] = value
                    } else {
                        // Expand with nulls.
                        while a.count < idx { a.append(.null) }
                        a.append(value)
                    }
                    return .array(a)
                }
                // If array but head isn't index, convert to object.
                return .object([head: value])
            default:
                return .object([head: value])
            }
        }

        // Descend.
        switch cur {
        case .object(var o):
            let nxt = o[head] ?? .object([:])
            o[head] = setRec(nxt, toks: tail, value: value)
            return .object(o)
        case .array(var a):
            if let idx = Int(head), idx >= 0 {
                if idx < a.count {
                    a[idx] = setRec(a[idx], toks: tail, value: value)
                } else {
                    while a.count < idx { a.append(.null) }
                    a.append(setRec(.object([:]), toks: tail, value: value))
                }
                return .array(a)
            }
            // If array but head isn't index, convert to object.
            return .object([head: setRec(.object([:]), toks: tail, value: value)])
        default:
            return .object([head: setRec(.object([:]), toks: tail, value: value)])
        }
    }
}

// MARK: - Adapter

public final class A2UIProtocolAdapter {
    private let enableV09: Bool
    private let preferred: A2UIProtocolVersion?

    public init(enableV09: Bool = true, preferredVersion: A2UIProtocolVersion? = nil) {
        self.enableV09 = enableV09
        self.preferred = preferredVersion
    }

    public func decode(line: String) throws -> [NormalizedMsg] {
        let trimmed = line.trimmingCharacters(in: .whitespacesAndNewlines)
        if trimmed.isEmpty {
            return []
        }

        guard let data = trimmed.data(using: .utf8) else {
            return [.error("Invalid UTF-8")]
        }

        let obj = try JSONSerialization.jsonObject(with: data, options: [])
        if let arr = obj as? [Any] {
            var out: [NormalizedMsg] = []
            for item in arr {
                out.append(contentsOf: normalize(item))
            }
            return out
        }
        return normalize(obj)
    }

    private func normalize(_ any: Any) -> [NormalizedMsg] {
        guard let dict = any as? [String: Any] else {
            return [.error("Expected object")]
        }
        // Messages are usually single-key objects.
        if let payload = dict["createSurface"] as? [String: Any] {
            return [normalizeCreateSurface(payload)]
        }
        if let payload = dict["deleteSurface"] as? [String: Any] {
            return [normalizeDeleteSurface(payload)]
        }
        if let payload = dict["updateComponents"] as? [String: Any] {
            return [normalizeUpdateComponents(payload)]
        }
        if let payload = dict["updateDataModel"] as? [String: Any] {
            return [normalizeUpdateDataModel(payload)]
        }
        return [.error("Unknown A2UI message keys: \(Array(dict.keys))")]
    }

    private func normalizeCreateSurface(_ payload: [String: Any]) -> NormalizedMsg {
        let surfaceId = (payload["surfaceId"] as? String) ?? (payload["surface_id"] as? String) ?? ""
        let catalogId = (payload["catalogId"] as? String) ?? (payload["catalog_id"] as? String)
        let root = (payload["rootComponentId"] as? String) ?? (payload["root_component_id"] as? String)
        return .createSurface(surfaceId: surfaceId, catalogId: catalogId, rootComponentId: root)
    }

    private func normalizeDeleteSurface(_ payload: [String: Any]) -> NormalizedMsg {
        let surfaceId = (payload["surfaceId"] as? String) ?? (payload["surface_id"] as? String) ?? ""
        return .deleteSurface(surfaceId: surfaceId)
    }

    private func normalizeUpdateComponents(_ payload: [String: Any]) -> NormalizedMsg {
        let surfaceId = (payload["surfaceId"] as? String) ?? (payload["surface_id"] as? String) ?? ""
        let compsAny = (payload["components"] as? [Any]) ?? []

        var comps: [NormalizedComponent] = []
        for cAny in compsAny {
            guard let cDict = cAny as? [String: Any] else { continue }
            let id = (cDict["id"] as? String) ?? (cDict["componentId"] as? String) ?? ""

            // v0.9 style: { id, component: "Text", ...props }
            if let t = cDict["component"] as? String {
                var props: [String: JSONValue] = [:]
                for (k, v) in cDict {
                    if k == "id" || k == "component" || k == "componentId" { continue }
                    props[k] = JSONValue.fromAny(v)
                }
                comps.append(NormalizedComponent(id: id, type: t, props: props))
                continue
            }

            // v0.8 style: { id, component: { Text: { ...props } } }
            if let wrapper = cDict["component"] as? [String: Any], wrapper.count == 1 {
                if let (t, innerAny) = wrapper.first {
                    let inner = innerAny as? [String: Any] ?? [:]
                    var props: [String: JSONValue] = [:]
                    for (k, v) in inner {
                        props[k] = JSONValue.fromAny(v)
                    }
                    comps.append(NormalizedComponent(id: id, type: t, props: props))
                    continue
                }
            }

            // Fallback: treat as unknown type.
            comps.append(NormalizedComponent(id: id, type: "Unknown", props: [:]))
        }

        return .updateComponents(surfaceId: surfaceId, components: comps)
    }

    private func normalizeUpdateDataModel(_ payload: [String: Any]) -> NormalizedMsg {
        let surfaceId = (payload["surfaceId"] as? String) ?? (payload["surface_id"] as? String) ?? ""

        // v0.9: updates: [{ path, value: <json> }]
        if let updatesAny = payload["updates"] as? [Any] {
            var ups: [NormalizedMsg.DataModelUpdate] = []
            for uAny in updatesAny {
                guard let u = uAny as? [String: Any] else { continue }
                let path = (u["path"] as? String) ?? ""
                let val = JSONValue.fromAny(u["value"])
                ups.append(.init(path: path, value: val))
            }
            return .updateDataModel(surfaceId: surfaceId, updates: ups)
        }

        // v0.8: contents: [{ path, value: {literalString: ...} }]
        if let contentsAny = payload["contents"] as? [Any] {
            var ups: [NormalizedMsg.DataModelUpdate] = []
            for uAny in contentsAny {
                guard let u = uAny as? [String: Any] else { continue }
                let path = (u["path"] as? String) ?? ""
                let val = normalizeLiteral(u["value"])
                ups.append(.init(path: path, value: val))
            }
            return .updateDataModel(surfaceId: surfaceId, updates: ups)
        }

        return .updateDataModel(surfaceId: surfaceId, updates: [])
    }

    private func normalizeLiteral(_ any: Any?) -> JSONValue {
        guard let dict = any as? [String: Any] else {
            return JSONValue.fromAny(any)
        }
        if let s = dict["literalString"] as? String {
            return .string(s)
        }
        if let b = dict["literalBoolean"] as? Bool {
            return .bool(b)
        }
        if let n = dict["literalNumber"] as? Double {
            return .number(n)
        }
        if dict.keys.contains("literalNull") {
            return .null
        }
        // Fallback: treat as JSON.
        return JSONValue.fromAny(dict)
    }
}
