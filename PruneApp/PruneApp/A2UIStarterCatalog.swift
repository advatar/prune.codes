import SwiftUI
import Combine
import A2UIRuntime

private extension Dictionary where Key == String, Value == JSONValue {
    func string(_ key: String) -> String? { self[key]?.stringValue }
    func bool(_ key: String) -> Bool? { self[key]?.boolValue }
    func number(_ key: String) -> Double? { self[key]?.numberValue }
    func object(_ key: String) -> [String: JSONValue]? { self[key]?.objectValue }
    func array(_ key: String) -> [JSONValue]? { self[key]?.arrayValue }
}

struct BoundValue {
    var path: String?
    var literalString: String?
    var literalNumber: Double?
    var literalBoolean: Bool?
    var literalArray: [JSONValue]?
    var literalNull: Bool?

    static func parse(_ value: JSONValue?) -> BoundValue? {
        guard let value else { return nil }
        switch value {
        case .string(let string):
            return BoundValue(path: nil, literalString: string, literalNumber: nil, literalBoolean: nil, literalArray: nil, literalNull: nil)
        case .number(let number):
            return BoundValue(path: nil, literalString: nil, literalNumber: number, literalBoolean: nil, literalArray: nil, literalNull: nil)
        case .bool(let bool):
            return BoundValue(path: nil, literalString: nil, literalNumber: nil, literalBoolean: bool, literalArray: nil, literalNull: nil)
        case .array(let array):
            return BoundValue(path: nil, literalString: nil, literalNumber: nil, literalBoolean: nil, literalArray: array, literalNull: nil)
        case .object(let obj):
            var bv = BoundValue()
            bv.path = obj["path"]?.stringValue
            bv.literalString = obj["literalString"]?.stringValue
            bv.literalNumber = obj["literalNumber"]?.numberValue
            bv.literalBoolean = obj["literalBoolean"]?.boolValue
            bv.literalArray = obj["literalArray"]?.arrayValue
            bv.literalNull = obj["literalNull"]?.boolValue
            if bv.path == nil,
               bv.literalString == nil,
               bv.literalNumber == nil,
               bv.literalBoolean == nil,
               bv.literalArray == nil,
               bv.literalNull == nil {
                return nil
            }
            return bv
        case .null:
            return BoundValue(path: nil, literalString: nil, literalNumber: nil, literalBoolean: nil, literalArray: nil, literalNull: true)
        }
    }

    func literalJSONValue() -> JSONValue? {
        if literalNull == true { return .null }
        if let s = literalString { return .string(s) }
        if let n = literalNumber { return .number(n) }
        if let b = literalBoolean { return .bool(b) }
        if let a = literalArray { return .array(a) }
        return nil
    }
}

enum ChildrenSpec {
    case explicitList([String])
    case template(dataBinding: String, componentId: String)

    static func parse(_ value: JSONValue?) -> ChildrenSpec? {
        guard case let .object(obj)? = value else { return nil }
        if let list = obj["explicitList"]?.arrayValue {
            let ids = list.compactMap { $0.stringValue }
            return .explicitList(ids)
        }
        if let template = obj["template"]?.objectValue,
           let binding = template["dataBinding"]?.stringValue,
           let componentId = template["componentId"]?.stringValue {
            return .template(dataBinding: binding, componentId: componentId)
        }
        return nil
    }
}

struct ActionContextEntry {
    let key: String
    let value: BoundValue
}

struct ActionDef {
    let name: String
    let context: [ActionContextEntry]
}

struct TextProps {
    var text: BoundValue
    var usageHint: String?

    static func parse(_ d: [String: JSONValue]) -> TextProps? {
        guard let text = BoundValue.parse(d["text"]) else { return nil }
        return TextProps(text: text, usageHint: d.string("usageHint"))
    }
}

struct ColumnProps {
    var children: ChildrenSpec?
    var distribution: String?
    var alignment: String?

    static func parse(_ d: [String: JSONValue]) -> ColumnProps {
        ColumnProps(
            children: ChildrenSpec.parse(d["children"]),
            distribution: d.string("distribution"),
            alignment: d.string("alignment")
        )
    }
}

struct RowProps {
    var children: ChildrenSpec?
    var distribution: String?
    var alignment: String?

    static func parse(_ d: [String: JSONValue]) -> RowProps {
        RowProps(
            children: ChildrenSpec.parse(d["children"]),
            distribution: d.string("distribution"),
            alignment: d.string("alignment")
        )
    }
}

struct ButtonProps {
    var child: String
    var primary: Bool
    var action: ActionDef?

    static func parse(_ d: [String: JSONValue]) -> ButtonProps? {
        guard let child = d.string("child") else { return nil }
        let primary = d.bool("primary") ?? false
        var actionDef: ActionDef?
        if let actionObj = d.object("action"),
           let name = actionObj["name"]?.stringValue {
            let ctxArray = actionObj["context"]?.arrayValue ?? []
            var ctx: [ActionContextEntry] = []
            for item in ctxArray {
                guard case let .object(entry) = item,
                      let key = entry["key"]?.stringValue,
                      let value = BoundValue.parse(entry["value"]) else { continue }
                ctx.append(ActionContextEntry(key: key, value: value))
            }
            actionDef = ActionDef(name: name, context: ctx)
        }
        return ButtonProps(child: child, primary: primary, action: actionDef)
    }
}

struct TextFieldProps {
    var label: BoundValue?
    var text: BoundValue?
    var placeholder: BoundValue?
    var textFieldType: String?

    static func parse(_ d: [String: JSONValue]) -> TextFieldProps {
        let textValue = d["text"] ?? d["value"]
        return TextFieldProps(
            label: BoundValue.parse(d["label"]),
            text: BoundValue.parse(textValue),
            placeholder: BoundValue.parse(d["placeholder"]),
            textFieldType: d.string("textFieldType")
        )
    }
}

struct CheckboxProps {
    var label: BoundValue?
    var value: BoundValue?

    static func parse(_ d: [String: JSONValue]) -> CheckboxProps {
        CheckboxProps(
            label: BoundValue.parse(d["label"]),
            value: BoundValue.parse(d["value"])
        )
    }
}

struct DividerProps {
    var axis: String?

    static func parse(_ d: [String: JSONValue]) -> DividerProps {
        DividerProps(axis: d.string("axis"))
    }
}

struct CardProps {
    var child: String?

    static func parse(_ d: [String: JSONValue]) -> CardProps {
        CardProps(child: d.string("child"))
    }
}

struct DateTimeInputProps {
    var label: BoundValue?
    var value: BoundValue?
    var enableDate: Bool
    var enableTime: Bool

    static func parse(_ d: [String: JSONValue]) -> DateTimeInputProps {
        DateTimeInputProps(
            label: BoundValue.parse(d["label"]),
            value: BoundValue.parse(d["value"]),
            enableDate: d.bool("enableDate") ?? false,
            enableTime: d.bool("enableTime") ?? false
        )
    }
}

enum A2UIComponent {
    case text(TextProps)
    case column(ColumnProps)
    case row(RowProps)
    case button(ButtonProps)
    case textField(TextFieldProps)
    case checkbox(CheckboxProps)
    case divider(DividerProps)
    case card(CardProps)
    case dateTimeInput(DateTimeInputProps)
    case unknown(type: String, raw: [String: JSONValue])
}

struct A2UIComponentInstance {
    let id: String
    let weight: Double?
    let component: A2UIComponent
}

enum A2UIStarterCatalogParser {
    static func parseComponentInstance(_ component: NormalizedComponent) -> A2UIComponentInstance {
        let props = component.props
        let weight = props["weight"]?.numberValue
        let type = component.kind

        let parsed: A2UIComponent
        switch type {
        case "Text":
            if let props = TextProps.parse(props) {
                parsed = .text(props)
            } else {
                parsed = .unknown(type: type, raw: props)
            }
        case "Column":
            parsed = .column(ColumnProps.parse(props))
        case "Row":
            parsed = .row(RowProps.parse(props))
        case "Button":
            if let props = ButtonProps.parse(props) {
                parsed = .button(props)
            } else {
                parsed = .unknown(type: type, raw: props)
            }
        case "TextField":
            parsed = .textField(TextFieldProps.parse(props))
        case "Checkbox":
            parsed = .checkbox(CheckboxProps.parse(props))
        case "Divider":
            parsed = .divider(DividerProps.parse(props))
        case "Card":
            parsed = .card(CardProps.parse(props))
        case "DateTimeInput":
            parsed = .dateTimeInput(DateTimeInputProps.parse(props))
        default:
            parsed = .unknown(type: type, raw: props)
        }

        return A2UIComponentInstance(id: component.id, weight: weight, component: parsed)
    }
}

@MainActor
protocol A2UIStarterCatalogStore: ObservableObject {
    func component(surfaceId: String, id: String) -> A2UIComponentInstance?
    func resolve(_ bound: BoundValue, surfaceId: String) -> JSONValue?
    func setValue(_ value: JSONValue, at path: String, surfaceId: String)
    func emitUserAction(name: String, surfaceId: String, sourceComponentId: String, context: [String: JSONValue])
}

@MainActor
final class A2UIStarterCatalogAdapter: ObservableObject, A2UIStarterCatalogStore {
    private let store: NormalizedSurfaceStore
    private let interactionHandler: ((A2UIUserActionEvent) -> Void)?
    private var seededDefaults: Set<String> = []
    private var cancellable: AnyCancellable?

    init(
        store: NormalizedSurfaceStore,
        interactionHandler: ((A2UIUserActionEvent) -> Void)?
    ) {
        self.store = store
        self.interactionHandler = interactionHandler
        self.cancellable = store.objectWillChange.sink { [weak self] _ in
            self?.objectWillChange.send()
        }
    }

    func component(surfaceId: String, id: String) -> A2UIComponentInstance? {
        guard let component = store.component(surfaceId: surfaceId, componentId: id) else { return nil }
        return A2UIStarterCatalogParser.parseComponentInstance(component)
    }

    func resolve(_ bound: BoundValue, surfaceId: String) -> JSONValue? {
        if let path = bound.path {
            if let current = store.dataModelValue(surfaceId: surfaceId, path: path), current != .null {
                return current
            }
            if let literal = bound.literalJSONValue() {
                let key = "\(surfaceId)|\(path)"
                if !seededDefaults.contains(key) {
                    seededDefaults.insert(key)
                    Task { @MainActor in
                        store.setDataModelValue(surfaceId: surfaceId, path: path, value: literal)
                    }
                }
                return literal
            }
            return nil
        }
        return bound.literalJSONValue()
    }

    func setValue(_ value: JSONValue, at path: String, surfaceId: String) {
        store.setDataModelValue(surfaceId: surfaceId, path: path, value: value)
    }

    func emitUserAction(
        name: String,
        surfaceId: String,
        sourceComponentId: String,
        context: [String: JSONValue]
    ) {
        guard let interactionHandler else { return }
        interactionHandler(A2UIUserActionEvent(
            surfaceId: surfaceId,
            componentId: sourceComponentId,
            name: name,
            context: context
        ))
    }
}

struct A2UIStarterCatalogSurfaceView: View {
    @StateObject private var adapter: A2UIStarterCatalogAdapter
    let surfaceId: String
    let rootComponentId: String

    init(
        store: NormalizedSurfaceStore,
        surfaceId: String,
        rootComponentId: String,
        interactionHandler: ((A2UIUserActionEvent) -> Void)?
    ) {
        _adapter = StateObject(wrappedValue: A2UIStarterCatalogAdapter(
            store: store,
            interactionHandler: interactionHandler
        ))
        self.surfaceId = surfaceId
        self.rootComponentId = rootComponentId
    }

    var body: some View {
        A2UIStarterCatalogComponentView(
            store: adapter,
            surfaceId: surfaceId,
            componentId: rootComponentId
        )
    }
}

struct A2UIStarterCatalogComponentView<Store: A2UIStarterCatalogStore>: View {
    @ObservedObject var store: Store
    let surfaceId: String
    let componentId: String

    var body: some View {
        if let instance = store.component(surfaceId: surfaceId, id: componentId) {
            render(instance: instance)
        } else {
            EmptyView()
        }
    }

    @ViewBuilder
    private func render(instance: A2UIComponentInstance) -> some View {
        switch instance.component {
        case .text(let props):
            let text = resolvedString(props.text) ?? ""
            Text(text).applyUsageHint(props.usageHint)

        case .column(let props):
            VStack(alignment: columnAlignment(props.alignment), spacing: 10) {
                renderChildren(props.children)
            }

        case .row(let props):
            HStack(alignment: rowAlignment(props.alignment), spacing: 10) {
                renderChildren(props.children)
            }

        case .button(let props):
            styledButton(primary: props.primary) {
                guard let action = props.action else { return }
                let ctx = resolveActionContext(action.context)
                store.emitUserAction(
                    name: action.name,
                    surfaceId: surfaceId,
                    sourceComponentId: instance.id,
                    context: ctx
                )
            } label: {
                A2UIStarterCatalogComponentView(store: store, surfaceId: surfaceId, componentId: props.child)
            }

        case .textField(let props):
            let labelText = (props.label.flatMap { resolvedString($0) }) ?? ""
            let binding = bindingString(props.text, fallbackLiteral: props.text?.literalString, componentId: instance.id)
            let type = props.textFieldType ?? "shortText"

            if type == "obscured" {
                SecureField(labelText, text: binding)
            } else if type == "longText" {
                VStack(alignment: .leading, spacing: 6) {
                    if !labelText.isEmpty {
                        Text(labelText).font(.caption).foregroundStyle(.secondary)
                    }
                    TextEditor(text: binding)
                        .frame(minHeight: 72)
                        .overlay(
                            RoundedRectangle(cornerRadius: 6)
                                .strokeBorder(Color.secondary.opacity(0.25), lineWidth: 1)
                        )
                }
            } else {
                TextField(labelText, text: binding)
            }

        case .checkbox(let props):
            let labelText = (props.label.flatMap { resolvedString($0) }) ?? ""
            Toggle(labelText, isOn: bindingBool(props.value, fallbackLiteral: props.value?.literalBoolean, componentId: instance.id))

        case .divider:
            Divider()

        case .card(let props):
            GroupBox {
                if let child = props.child {
                    A2UIStarterCatalogComponentView(store: store, surfaceId: surfaceId, componentId: child)
                }
            }

        case .dateTimeInput(let props):
            dateTimeInput(props, componentId: instance.id)

        case .unknown(let type, _):
            Text("Unsupported component: \(type)")
                .font(.footnote)
                .foregroundStyle(.secondary)
        }
    }

    @ViewBuilder
    private func renderChildren(_ children: ChildrenSpec?) -> some View {
        switch children {
        case .explicitList(let ids):
            ForEach(ids, id: \.self) { id in
                A2UIStarterCatalogComponentView(store: store, surfaceId: surfaceId, componentId: id)
            }
        case .template:
            Text("Template children not implemented in starter catalog.")
                .font(.footnote)
                .foregroundStyle(.secondary)
        case .none:
            EmptyView()
        }
    }

    private func resolvedString(_ bv: BoundValue) -> String? {
        if let value = store.resolve(bv, surfaceId: surfaceId) {
            if let s = value.stringValue { return s }
            if let n = value.numberValue { return String(n) }
            if let b = value.boolValue { return b ? "true" : "false" }
        }
        return bv.literalString
    }

    private func bindingString(
        _ bv: BoundValue?,
        fallbackLiteral: String?,
        componentId: String
    ) -> Binding<String> {
        guard let bv else {
            return Binding(get: { fallbackLiteral ?? "" }, set: { _ in })
        }
        if let path = bv.path {
            return Binding(
                get: {
                    if let resolved = store.resolve(bv, surfaceId: surfaceId) {
                        return resolved.stringValue ?? fallbackLiteral ?? ""
                    }
                    return fallbackLiteral ?? ""
                },
                set: { newValue in
                    store.setValue(.string(newValue), at: path, surfaceId: surfaceId)
                    store.emitUserAction(
                        name: "input.change",
                        surfaceId: surfaceId,
                        sourceComponentId: componentId,
                        context: inputContext(path: path, value: .string(newValue))
                    )
                }
            )
        }
        return Binding(get: { bv.literalString ?? fallbackLiteral ?? "" }, set: { _ in })
    }

    private func bindingBool(
        _ bv: BoundValue?,
        fallbackLiteral: Bool?,
        componentId: String
    ) -> Binding<Bool> {
        guard let bv else {
            return Binding(get: { fallbackLiteral ?? false }, set: { _ in })
        }
        if let path = bv.path {
            return Binding(
                get: {
                    if let resolved = store.resolve(bv, surfaceId: surfaceId) {
                        return resolved.boolValue ?? fallbackLiteral ?? false
                    }
                    return fallbackLiteral ?? false
                },
                set: { newValue in
                    store.setValue(.bool(newValue), at: path, surfaceId: surfaceId)
                    store.emitUserAction(
                        name: "input.change",
                        surfaceId: surfaceId,
                        sourceComponentId: componentId,
                        context: inputContext(path: path, value: .bool(newValue))
                    )
                }
            )
        }
        return Binding(get: { bv.literalBoolean ?? fallbackLiteral ?? false }, set: { _ in })
    }

    private func resolveActionContext(_ entries: [ActionContextEntry]) -> [String: JSONValue] {
        var out: [String: JSONValue] = [:]
        for entry in entries {
            out[entry.key] = store.resolve(entry.value, surfaceId: surfaceId) ?? .null
        }
        return out
    }

    @ViewBuilder
    private func dateTimeInput(_ props: DateTimeInputProps, componentId: String) -> some View {
        let label = props.label.flatMap { resolvedString($0) } ?? ""
        if !props.enableDate && !props.enableTime {
            Text(label.isEmpty ? "Invalid DateTimeInput" : label)
                .font(.footnote)
                .foregroundStyle(.secondary)
        } else {
            let binding = bindingDate(
                props.value,
                enableDate: props.enableDate,
                enableTime: props.enableTime,
                componentId: componentId
            )
            DatePicker(
                label,
                selection: binding,
                displayedComponents: displayedComponents(enableDate: props.enableDate, enableTime: props.enableTime)
            )
        }
    }

    private func displayedComponents(enableDate: Bool, enableTime: Bool) -> DatePickerComponents {
        switch (enableDate, enableTime) {
        case (true, true): return [.date, .hourAndMinute]
        case (true, false): return [.date]
        case (false, true): return [.hourAndMinute]
        default: return [.date]
        }
    }

    private func bindingDate(
        _ bv: BoundValue?,
        enableDate: Bool,
        enableTime: Bool,
        componentId: String
    ) -> Binding<Date> {
        guard let bv else { return .constant(Date()) }
        guard let path = bv.path else {
            let d = parseA2UIDate(from: bv.literalString) ?? Date()
            return Binding(get: { d }, set: { _ in })
        }

        return Binding(
            get: {
                if let resolved = store.resolve(bv, surfaceId: surfaceId) {
                    if let s = resolved.stringValue { return parseA2UIDate(from: s) ?? Date() }
                }
                if let fallback = bv.literalString {
                    return parseA2UIDate(from: fallback) ?? Date()
                }
                return Date()
            },
            set: { newDate in
                let encoded = encodeA2UIDate(newDate, enableDate: enableDate, enableTime: enableTime)
                store.setValue(.string(encoded), at: path, surfaceId: surfaceId)
                store.emitUserAction(
                    name: "input.change",
                    surfaceId: surfaceId,
                    sourceComponentId: componentId,
                    context: inputContext(path: path, value: .string(encoded))
                )
            }
        )
    }

    private func parseA2UIDate(from s: String?) -> Date? {
        guard let s else { return nil }
        if s.count == 10,
           s[s.index(s.startIndex, offsetBy: 4)] == "-",
           s[s.index(s.startIndex, offsetBy: 7)] == "-" {
            let parts = s.split(separator: "-")
            guard parts.count == 3,
                  let y = Int(parts[0]),
                  let m = Int(parts[1]),
                  let d = Int(parts[2]) else { return nil }
            return Calendar.current.date(from: DateComponents(year: y, month: m, day: d))
        }

        if s.count == 5, s[s.index(s.startIndex, offsetBy: 2)] == ":" {
            let parts = s.split(separator: ":")
            guard parts.count == 2,
                  let hh = Int(parts[0]),
                  let mm = Int(parts[1]) else { return nil }
            let today = Calendar.current.dateComponents([.year, .month, .day], from: Date())
            return Calendar.current.date(from: DateComponents(year: today.year, month: today.month, day: today.day, hour: hh, minute: mm))
        }

        let iso = ISO8601DateFormatter()
        iso.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        if let d = iso.date(from: s) { return d }
        iso.formatOptions = [.withInternetDateTime]
        return iso.date(from: s)
    }

    private func encodeA2UIDate(_ date: Date, enableDate: Bool, enableTime: Bool) -> String {
        if enableDate && !enableTime {
            let ymd = DateFormatter()
            ymd.calendar = Calendar(identifier: .iso8601)
            ymd.locale = Locale(identifier: "en_US_POSIX")
            ymd.timeZone = TimeZone(secondsFromGMT: 0)
            ymd.dateFormat = "yyyy-MM-dd"
            return ymd.string(from: date)
        }
        if !enableDate && enableTime {
            let hm = DateFormatter()
            hm.calendar = Calendar(identifier: .iso8601)
            hm.locale = Locale(identifier: "en_US_POSIX")
            hm.timeZone = TimeZone(secondsFromGMT: 0)
            hm.dateFormat = "HH:mm"
            return hm.string(from: date)
        }
        let iso = ISO8601DateFormatter()
        iso.formatOptions = [.withInternetDateTime]
        return iso.string(from: date)
    }

    private func inputContext(path: String?, value: JSONValue) -> [String: JSONValue] {
        var context: [String: JSONValue] = ["value": value]
        if let path, !path.isEmpty {
            context["path"] = .string(path)
        }
        return context
    }

    @ViewBuilder
    private func styledButton(
        primary: Bool,
        action: @escaping () -> Void,
        @ViewBuilder label: () -> some View
    ) -> some View {
        if #available(macOS 12.0, *) {
            if primary {
                Button(action: action, label: label)
                    .buttonStyle(BorderedProminentButtonStyle())
            } else {
                Button(action: action, label: label)
                    .buttonStyle(BorderedButtonStyle())
            }
        } else {
            Button(action: action, label: label)
                .buttonStyle(PlainButtonStyle())
        }
    }

    private func columnAlignment(_ s: String?) -> HorizontalAlignment {
        switch s {
        case "start": return .leading
        case "center": return .center
        case "end": return .trailing
        default: return .leading
        }
    }

    private func rowAlignment(_ s: String?) -> VerticalAlignment {
        switch s {
        case "start": return .top
        case "center": return .center
        case "end": return .bottom
        default: return .center
        }
    }
}

private extension Text {
    func applyUsageHint(_ hint: String?) -> some View {
        switch hint {
        case "h1": return self.font(.largeTitle).bold()
        case "h2": return self.font(.title).bold()
        case "h3": return self.font(.title2).bold()
        case "h4": return self.font(.title3).bold()
        case "h5": return self.font(.headline)
        case "caption": return self.font(.caption)
        case "body": return self.font(.body)
        default: return self.font(.body)
        }
    }
}
