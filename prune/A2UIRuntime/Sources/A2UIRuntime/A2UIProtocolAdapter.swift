import Foundation

public struct A2UIProtocolAdapter: Sendable {
    public var enableV09: Bool
    public var preferredVersion: A2UIProtocolVersion?

    public init(enableV09: Bool = true, preferredVersion: A2UIProtocolVersion? = nil) {
        self.enableV09 = enableV09
        self.preferredVersion = preferredVersion
    }

    public func decode(line: String) -> [NormalizedMsg] {
        let trimmed = line.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return [] }

        do {
            guard let data = trimmed.data(using: .utf8) else {
                return [.error("invalid UTF-8 input")]
            }
            let json = try JSONValue.parse(data: data)
            return decode(json: json)
        } catch {
            return [.error("failed to parse JSON: \(error)")]
        }
    }

    func decode(json: JSONValue) -> [NormalizedMsg] {
        guard case let .object(obj) = json else {
            return [.error("expected JSON object for A2UI message")]
        }

        let version = detectVersion(keys: Set(obj.keys), preferredVersion: preferredVersion)
        if version == .v09 && !enableV09 {
            return [.error("v0.9 messages are disabled in this adapter")]
        }

        switch version {
        case .v08:
            return decodeV08(obj)
        case .v09:
            return decodeV09(obj)
        }
    }

    private func detectVersion(
        keys: Set<String>,
        preferredVersion: A2UIProtocolVersion?
    ) -> A2UIProtocolVersion {
        let v09Keys: Set<String> = ["createSurface", "updateComponents", "updateDataModel"]
        let v08Keys: Set<String> = ["beginRendering", "surfaceUpdate", "dataModelUpdate"]
        if !v09Keys.isDisjoint(with: keys) {
            return .v09
        }
        if !v08Keys.isDisjoint(with: keys) {
            return .v08
        }
        return preferredVersion ?? .v09
    }

    private func decodeV08(_ obj: [String: JSONValue]) -> [NormalizedMsg] {
        var messages: [NormalizedMsg] = []

        if let payload = obj["beginRendering"] {
            messages.append(decodeBeginRendering(payload: payload, version: .v08))
        }
        if let payload = obj["surfaceUpdate"] {
            messages.append(contentsOf: decodeSurfaceUpdate(payload: payload))
        }
        if let payload = obj["dataModelUpdate"] {
            messages.append(contentsOf: decodeDataModelUpdateV08(payload: payload))
        }
        if let payload = obj["deleteSurface"] {
            messages.append(contentsOf: decodeDeleteSurface(payload: payload))
        }

        if messages.isEmpty {
            return [.error("unsupported v0.8 message keys: \(obj.keys.sorted())")]
        }
        return messages
    }

    private func decodeV09(_ obj: [String: JSONValue]) -> [NormalizedMsg] {
        var messages: [NormalizedMsg] = []

        if let payload = obj["createSurface"] {
            messages.append(decodeBeginRendering(payload: payload, version: .v09))
        }
        if let payload = obj["updateComponents"] {
            messages.append(contentsOf: decodeUpdateComponentsV09(payload: payload))
        }
        if let payload = obj["updateDataModel"] {
            messages.append(contentsOf: decodeDataModelUpdateV09(payload: payload))
        }
        if let payload = obj["deleteSurface"] {
            messages.append(contentsOf: decodeDeleteSurface(payload: payload))
        }

        if messages.isEmpty {
            return [.error("unsupported v0.9 message keys: \(obj.keys.sorted())")]
        }
        return messages
    }

    private func decodeBeginRendering(payload: JSONValue, version: A2UIProtocolVersion) -> NormalizedMsg {
        guard let dict = payload.objectValue,
              let surfaceId = dict["surfaceId"]?.stringValue else {
            return .error("missing surfaceId in createSurface/beginRendering")
        }

        let catalogId = dict["catalogId"]?.stringValue
        let rootComponentId = dict["rootComponentId"]?.stringValue

        return .createSurface(NormalizedSurfaceInfo(
            surfaceId: surfaceId,
            catalogId: catalogId,
            rootComponentId: rootComponentId,
            protocolVersion: version
        ))
    }

    private func decodeSurfaceUpdate(payload: JSONValue) -> [NormalizedMsg] {
        guard let dict = payload.objectValue,
              let surfaceId = dict["surfaceId"]?.stringValue else {
            return [.error("missing surfaceId in surfaceUpdate")]
        }

        let components = (dict["components"]?.arrayValue ?? []).compactMap { decodeComponentV08($0) }
        return [.updateComponents(surfaceId: surfaceId, components: components)]
    }

    private func decodeUpdateComponentsV09(payload: JSONValue) -> [NormalizedMsg] {
        guard let dict = payload.objectValue,
              let surfaceId = dict["surfaceId"]?.stringValue else {
            return [.error("missing surfaceId in updateComponents")]
        }

        let components = (dict["components"]?.arrayValue ?? []).compactMap { decodeComponentV09($0) }
        return [.updateComponents(surfaceId: surfaceId, components: components)]
    }

    private func decodeDataModelUpdateV08(payload: JSONValue) -> [NormalizedMsg] {
        guard let dict = payload.objectValue,
              let surfaceId = dict["surfaceId"]?.stringValue else {
            return [.error("missing surfaceId in dataModelUpdate")]
        }

        let updates: [NormalizedDataUpdate] = (dict["contents"]?.arrayValue ?? []).compactMap { entry in
            guard let obj = entry.objectValue,
                  let path = obj["path"]?.stringValue else {
                return nil
            }
            let rawValue = obj["value"] ?? .object(obj)
            let value = A2UIValueParser.literalValue(from: rawValue) ?? rawValue
            return NormalizedDataUpdate(path: path, value: value)
        }

        return [.updateDataModel(surfaceId: surfaceId, updates: updates)]
    }

    private func decodeDataModelUpdateV09(payload: JSONValue) -> [NormalizedMsg] {
        guard let dict = payload.objectValue,
              let surfaceId = dict["surfaceId"]?.stringValue else {
            return [.error("missing surfaceId in updateDataModel")]
        }

        let updates: [NormalizedDataUpdate] = (dict["updates"]?.arrayValue ?? []).compactMap { entry in
            guard let obj = entry.objectValue,
                  let path = obj["path"]?.stringValue else {
                return nil
            }
            let value = obj["value"] ?? .null
            let metadata = obj["metadata"]
            return NormalizedDataUpdate(path: path, value: value, metadata: metadata)
        }

        return [.updateDataModel(surfaceId: surfaceId, updates: updates)]
    }

    private func decodeDeleteSurface(payload: JSONValue) -> [NormalizedMsg] {
        guard let dict = payload.objectValue,
              let surfaceId = dict["surfaceId"]?.stringValue else {
            return [.error("missing surfaceId in deleteSurface")]
        }
        return [.deleteSurface(surfaceId: surfaceId)]
    }

    private func decodeComponentV08(_ value: JSONValue) -> NormalizedComponent? {
        guard let obj = value.objectValue,
              let id = obj["id"]?.stringValue,
              let wrapper = obj["component"]?.objectValue,
              let (kind, propsValue) = wrapper.first else {
            return nil
        }

        let props = propsValue.objectValue ?? [:]
        return normalizeComponent(id: id, kind: kind, props: props)
    }

    private func decodeComponentV09(_ value: JSONValue) -> NormalizedComponent? {
        guard let obj = value.objectValue,
              let id = obj["id"]?.stringValue,
              let kind = obj["component"]?.stringValue else {
            return nil
        }

        var props = obj
        props.removeValue(forKey: "id")
        props.removeValue(forKey: "component")

        return normalizeComponent(id: id, kind: kind, props: props)
    }

    private func normalizeComponent(id: String, kind: String, props: [String: JSONValue]) -> NormalizedComponent {
        let childrenRefs = props["children"]?.arrayValue?.compactMap { $0.stringValue } ?? []
        let childRef = props["child"]?.stringValue

        return NormalizedComponent(
            id: id,
            kind: kind,
            props: props,
            childrenRefs: childrenRefs,
            childRef: childRef
        )
    }
}
