import Foundation

public enum JSONValue: Sendable, Equatable {
    case object([String: JSONValue])
    case array([JSONValue])
    case string(String)
    case number(Double)
    case bool(Bool)
    case null
}

public enum JSONValueError: Error, CustomStringConvertible {
    case invalidTopLevel

    public var description: String {
        switch self {
        case .invalidTopLevel:
            return "invalid JSON value"
        }
    }
}

public extension JSONValue {
    init(fromAny any: Any?) {
        self = JSONValue.fromAny(any)
    }

    init(jsonObject: Any) throws {
        if let dict = jsonObject as? [String: Any] {
            var obj: [String: JSONValue] = [:]
            for (key, value) in dict {
                obj[key] = try JSONValue(jsonObject: value)
            }
            self = .object(obj)
        } else if let array = jsonObject as? [Any] {
            let values = try array.map { try JSONValue(jsonObject: $0) }
            self = .array(values)
        } else if let string = jsonObject as? String {
            self = .string(string)
        } else if let number = jsonObject as? NSNumber {
            if CFGetTypeID(number) == CFBooleanGetTypeID() {
                self = .bool(number.boolValue)
            } else {
                self = .number(number.doubleValue)
            }
        } else if jsonObject is NSNull {
            self = .null
        } else {
            throw JSONValueError.invalidTopLevel
        }
    }

    static func fromAny(_ any: Any?) -> JSONValue {
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
        case let n as NSNumber:
            if CFGetTypeID(n) == CFBooleanGetTypeID() {
                return .bool(n.boolValue)
            }
            return .number(n.doubleValue)
        case let s as String:
            return .string(s)
        case let a as [Any]:
            return .array(a.map { JSONValue.fromAny($0) })
        case let o as [String: Any]:
            var out: [String: JSONValue] = [:]
            for (key, value) in o {
                out[key] = JSONValue.fromAny(value)
            }
            return .object(out)
        default:
            return .string(String(describing: any))
        }
    }

    static func parse(data: Data) throws -> JSONValue {
        let obj = try JSONSerialization.jsonObject(with: data, options: [])
        return try JSONValue(jsonObject: obj)
    }

    var objectValue: [String: JSONValue]? {
        if case let .object(obj) = self { return obj }
        return nil
    }

    var arrayValue: [JSONValue]? {
        if case let .array(array) = self { return array }
        return nil
    }

    var stringValue: String? {
        if case let .string(value) = self { return value }
        return nil
    }

    var numberValue: Double? {
        if case let .number(value) = self { return value }
        return nil
    }

    var boolValue: Bool? {
        if case let .bool(value) = self { return value }
        return nil
    }

    func toAny() -> Any {
        switch self {
        case .object(let obj):
            var out: [String: Any] = [:]
            for (key, value) in obj {
                out[key] = value.toAny()
            }
            return out
        case .array(let array):
            return array.map { $0.toAny() }
        case .string(let value):
            return value
        case .number(let value):
            return value
        case .bool(let value):
            return value
        case .null:
            return NSNull()
        }
    }
}

public extension JSONValue {
    func value(at pointer: String) -> JSONValue? {
        let tokens = JSONPointer.tokens(from: pointer)
        return value(at: tokens)
    }

    func value(at tokens: [String]) -> JSONValue? {
        guard !tokens.isEmpty else { return self }
        let head = tokens[0]
        let rest = Array(tokens.dropFirst())

        switch self {
        case .object(let obj):
            guard let next = obj[head] else { return nil }
            return next.value(at: rest)
        case .array(let array):
            guard let index = Int(head), array.indices.contains(index) else { return nil }
            return array[index].value(at: rest)
        default:
            return nil
        }
    }

    mutating func setValue(_ value: JSONValue, at pointer: String) {
        let tokens = JSONPointer.tokens(from: pointer)
        self = setValue(value, tokens: tokens)
    }

    private func setValue(_ value: JSONValue, tokens: [String]) -> JSONValue {
        guard let head = tokens.first else {
            return value
        }

        let rest = Array(tokens.dropFirst())

        switch self {
        case .object(var obj):
            let child = obj[head] ?? .object([:])
            obj[head] = child.setValue(value, tokens: rest)
            return .object(obj)
        case .array(var array):
            guard let index = Int(head) else {
                return .array(array)
            }
            if index >= array.count {
                array.append(contentsOf: Array(repeating: .null, count: index - array.count + 1))
            }
            array[index] = array[index].setValue(value, tokens: rest)
            return .array(array)
        default:
            if Int(head) != nil {
                var array: [JSONValue] = []
                if let index = Int(head) {
                    array.append(contentsOf: Array(repeating: .null, count: index + 1))
                    array[index] = .null.setValue(value, tokens: rest)
                }
                return .array(array)
            } else {
                var obj: [String: JSONValue] = [:]
                obj[head] = .object([:]).setValue(value, tokens: rest)
                return .object(obj)
            }
        }
    }
}
