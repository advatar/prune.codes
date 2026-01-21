import Foundation

public enum NormalizedDynamicValue: Sendable, Equatable {
    case literal(JSONValue)
    case binding(path: String)
}

public enum A2UIValueParser {
    static func literalValue(from value: JSONValue) -> JSONValue? {
        guard case let .object(obj) = value else { return nil }

        if let v = obj["literalString"]?.stringValue { return .string(v) }
        if let v = obj["literalBoolean"]?.boolValue { return .bool(v) }
        if let v = obj["literalNumber"]?.numberValue { return .number(v) }
        if obj["literalNull"] != nil { return .null }

        if let v = obj["valueString"]?.stringValue { return .string(v) }
        if let v = obj["valueBoolean"]?.boolValue { return .bool(v) }
        if let v = obj["valueNumber"]?.numberValue { return .number(v) }
        if obj["valueNull"] != nil { return .null }

        if let list = obj["valueList"]?.arrayValue {
            let mapped = list.map { element in
                literalValue(from: element) ?? element
            }
            return .array(mapped)
        }

        if let map = obj["valueMap"]?.objectValue {
            var out: [String: JSONValue] = [:]
            for (key, element) in map {
                out[key] = literalValue(from: element) ?? element
            }
            return .object(out)
        }

        return nil
    }
}

public enum NormalizedValueResolver {
    public static func normalize(value: JSONValue) -> NormalizedDynamicValue {
        if case let .object(obj) = value,
           let path = obj["path"]?.stringValue {
            return .binding(path: path)
        }
        if let literal = A2UIValueParser.literalValue(from: value) {
            return .literal(literal)
        }
        return .literal(value)
    }

    public static func resolve(value: JSONValue, dataModel: JSONValue) -> JSONValue? {
        let normalized = normalize(value: value)
        return resolve(normalized: normalized, dataModel: dataModel)
    }

    public static func resolve(normalized: NormalizedDynamicValue, dataModel: JSONValue) -> JSONValue? {
        switch normalized {
        case .literal(let literal):
            return literal
        case .binding(let path):
            return dataModel.value(at: path)
        }
    }

    public static func resolveBindings(in props: [String: JSONValue], dataModel: JSONValue) -> [String: JSONValue] {
        var out: [String: JSONValue] = [:]
        for (key, value) in props {
            if let resolved = resolve(value: value, dataModel: dataModel) {
                out[key] = resolved
            } else {
                out[key] = value
            }
        }
        return out
    }
}
