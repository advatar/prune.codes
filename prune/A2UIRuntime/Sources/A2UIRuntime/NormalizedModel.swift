import Foundation

public enum A2UIProtocolVersion: String, Sendable {
    case v08
    case v09
}

public struct NormalizedComponent: Sendable, Equatable {
    public let id: String
    public let kind: String
    public let props: [String: JSONValue]
    public let childrenRefs: [String]
    public let childRef: String?

    public init(
        id: String,
        kind: String,
        props: [String: JSONValue],
        childrenRefs: [String],
        childRef: String?
    ) {
        self.id = id
        self.kind = kind
        self.props = props
        self.childrenRefs = childrenRefs
        self.childRef = childRef
    }
}

public struct NormalizedDataUpdate: Sendable, Equatable {
    public let path: String
    public let value: JSONValue
    public let metadata: JSONValue?

    public init(path: String, value: JSONValue, metadata: JSONValue? = nil) {
        self.path = path
        self.value = value
        self.metadata = metadata
    }
}

public struct NormalizedSurfaceInfo: Sendable, Equatable {
    public let surfaceId: String
    public let catalogId: String?
    public let rootComponentId: String?
    public let protocolVersion: A2UIProtocolVersion?

    public init(
        surfaceId: String,
        catalogId: String?,
        rootComponentId: String?,
        protocolVersion: A2UIProtocolVersion?
    ) {
        self.surfaceId = surfaceId
        self.catalogId = catalogId
        self.rootComponentId = rootComponentId
        self.protocolVersion = protocolVersion
    }
}

public enum NormalizedMsg: Sendable, Equatable {
    case createSurface(NormalizedSurfaceInfo)
    case updateComponents(surfaceId: String, components: [NormalizedComponent])
    case updateDataModel(surfaceId: String, updates: [NormalizedDataUpdate])
    case deleteSurface(surfaceId: String)
    case error(String)
}

@MainActor
public final class NormalizedSurfaceStore: ObservableObject {
    public struct Surface: Sendable, Equatable {
        public var info: NormalizedSurfaceInfo
        public var components: [String: NormalizedComponent]
        public var dataModel: JSONValue

        public init(info: NormalizedSurfaceInfo) {
            self.info = info
            self.components = [:]
            self.dataModel = .object([:])
        }
    }

    @Published public private(set) var surfaces: [String: Surface] = [:]
    @Published public private(set) var lastError: String?

    public init() {}

    public func apply(_ msg: NormalizedMsg) {
        switch msg {
        case .createSurface(let info):
            var surface = surfaces[info.surfaceId] ?? Surface(info: info)
            surface.info = info
            surfaces[info.surfaceId] = surface
        case .updateComponents(let surfaceId, let components):
            var surface = surfaces[surfaceId] ?? Surface(info: NormalizedSurfaceInfo(
                surfaceId: surfaceId,
                catalogId: nil,
                rootComponentId: nil,
                protocolVersion: nil
            ))
            for component in components {
                surface.components[component.id] = component
            }
            surfaces[surfaceId] = surface
        case .updateDataModel(let surfaceId, let updates):
            var surface = surfaces[surfaceId] ?? Surface(info: NormalizedSurfaceInfo(
                surfaceId: surfaceId,
                catalogId: nil,
                rootComponentId: nil,
                protocolVersion: nil
            ))
            var model = surface.dataModel
            for update in updates {
                model.setValue(update.value, at: update.path)
            }
            surface.dataModel = model
            surfaces[surfaceId] = surface
        case .deleteSurface(let surfaceId):
            surfaces.removeValue(forKey: surfaceId)
        case .error(let message):
            lastError = message
        }
    }

    public func apply(_ messages: [NormalizedMsg]) {
        for message in messages {
            apply(message)
        }
    }

    public func rootComponentId(for surfaceId: String) -> String? {
        guard let surface = surfaces[surfaceId] else { return nil }
        let allIds = Set(surface.components.keys)
        var referenced: Set<String> = []
        for component in surface.components.values {
            referenced.formUnion(component.childrenRefs)
            if let child = component.childRef {
                referenced.insert(child)
            }
        }
        let candidates = allIds.subtracting(referenced)
        if candidates.count == 1 {
            return candidates.first
        }
        if let root = surface.info.rootComponentId {
            return root
        }
        return allIds.sorted().first
    }

    public func resolvedProps(surfaceId: String, componentId: String) -> [String: JSONValue]? {
        guard let surface = surfaces[surfaceId],
              let component = surface.components[componentId] else {
            return nil
        }
        return NormalizedValueResolver.resolveBindings(in: component.props, dataModel: surface.dataModel)
    }

    public func resolveDynamicValue(surfaceId: String, value: JSONValue) -> JSONValue? {
        guard let surface = surfaces[surfaceId] else { return nil }
        return NormalizedValueResolver.resolve(value: value, dataModel: surface.dataModel)
    }
}
