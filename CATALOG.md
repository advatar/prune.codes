Here’s a **starter “catalog” for A2UI v0.8** that covers what the Quickstart’s simplified payload uses (**Text**, **DateTimeInput**, **Button**) and adds a few practical basics (**Column**, **Row**, **TextField**, **Checkbox**, **Divider**, **Card**).  [oai_citation:0‡A2UI](https://a2ui.org/quickstart/)

```swift
//
//  A2UIStarterCatalog.swift
//
//  Minimal A2UI v0.8 catalog + SwiftUI renderer for:
//  Layout: Column, Row
//  Display: Text, Divider
//  Container: Card
//  Inputs: Button, TextField, Checkbox, DateTimeInput
//
//  Notes (v0.8):
//  - Components arrive via surfaceUpdate as a flat adjacency list (id references).
//  - BoundValue supports literal*, path, or both (path+literal => initialization shorthand).
//  - Button.action.context is an ARRAY of {key,value}; userAction.context sent to server is an OBJECT
//    after resolving those BoundValues.
//  See spec/quickstart for examples.
//

import SwiftUI
import Foundation

// MARK: - Small JSON helpers

private extension Dictionary where Key == String, Value == Any {
    func str(_ key: String) -> String? { self[key] as? String }
    func bool(_ key: String) -> Bool? { self[key] as? Bool }
    func dbl(_ key: String) -> Double? {
        if let d = self[key] as? Double { return d }
        if let i = self[key] as? Int { return Double(i) }
        return nil
    }
    func int(_ key: String) -> Int? {
        if let i = self[key] as? Int { return i }
        if let d = self[key] as? Double { return Int(d) }
        return nil
    }
    func dict(_ key: String) -> [String: Any]? { self[key] as? [String: Any] }
    func arr(_ key: String) -> [Any]? { self[key] as? [Any] }
}

// MARK: - BoundValue (v0.8)

/// A value that can be literal*, path, or both.
/// Supported literals in v0.8: literalString/literalNumber/literalBoolean/literalArray.
struct BoundValue {
    var path: String?
    var literalString: String?
    var literalNumber: Double?
    var literalBoolean: Bool?
    var literalArray: [Any]?

    static func parse(_ any: Any?) -> BoundValue? {
        guard let d = any as? [String: Any] else { return nil }
        var bv = BoundValue()
        bv.path = d.str("path")
        bv.literalString = d.str("literalString")
        bv.literalNumber = d.dbl("literalNumber")
        bv.literalBoolean = d.bool("literalBoolean")
        bv.literalArray = d.arr("literalArray")
        // Spec says minProperties: 1, so return nil if totally empty.
        if bv.path == nil, bv.literalString == nil, bv.literalNumber == nil, bv.literalBoolean == nil, bv.literalArray == nil {
            return nil
        }
        return bv
    }

    /// Returns the first literal* present, as Any (String/Double/Bool/[Any]).
    func literalAny() -> Any? {
        if let s = literalString { return s }
        if let n = literalNumber { return n }
        if let b = literalBoolean { return b }
        if let a = literalArray { return a }
        return nil
    }
}

// MARK: - Children (v0.8)

enum ChildrenSpec {
    case explicitList([String])
    case template(dataBinding: String, componentId: String)

    static func parse(_ any: Any?) -> ChildrenSpec? {
        guard let d = any as? [String: Any] else { return nil }

        if let list = d.arr("explicitList") as? [String] {
            return .explicitList(list)
        }

        if let tmpl = d.dict("template"),
           let binding = tmpl.str("dataBinding"),
           let componentId = tmpl.str("componentId") {
            return .template(dataBinding: binding, componentId: componentId)
        }

        return nil
    }
}

// MARK: - Action (v0.8)

struct ActionContextEntry {
    let key: String
    let value: BoundValue
}

struct ActionDef {
    let name: String
    let context: [ActionContextEntry]
}

// MARK: - Component props

struct TextProps {
    var text: BoundValue
    var usageHint: String?
    static func parse(_ d: [String: Any]) -> TextProps? {
        guard let text = BoundValue.parse(d["text"]) else { return nil }
        return TextProps(text: text, usageHint: d.str("usageHint"))
    }
}

struct ColumnProps {
    var children: ChildrenSpec?
    var distribution: String?   // start/center/end/spaceBetween/...
    var alignment: String?      // start/center/end/stretch
    static func parse(_ d: [String: Any]) -> ColumnProps {
        ColumnProps(
            children: ChildrenSpec.parse(d["children"]),
            distribution: d.str("distribution"),
            alignment: d.str("alignment")
        )
    }
}

struct RowProps {
    var children: ChildrenSpec?
    var distribution: String?
    var alignment: String?
    static func parse(_ d: [String: Any]) -> RowProps {
        RowProps(
            children: ChildrenSpec.parse(d["children"]),
            distribution: d.str("distribution"),
            alignment: d.str("alignment")
        )
    }
}

struct ButtonProps {
    var child: String
    var primary: Bool
    var action: ActionDef?
    static func parse(_ d: [String: Any]) -> ButtonProps? {
        guard let child = d.str("child") else { return nil }
        let primary = d.bool("primary") ?? false

        var actionDef: ActionDef?
        if let a = d.dict("action"), let name = a.str("name") {
            let ctxArray = (a.arr("context") ?? [])
            var ctx: [ActionContextEntry] = []
            for item in ctxArray {
                guard let entry = item as? [String: Any],
                      let k = entry.str("key"),
                      let v = BoundValue.parse(entry["value"]) else { continue }
                ctx.append(ActionContextEntry(key: k, value: v))
            }
            actionDef = ActionDef(name: name, context: ctx)
        }

        return ButtonProps(child: child, primary: primary, action: actionDef)
    }
}

struct TextFieldProps {
    var label: BoundValue?
    var text: BoundValue?            // usually path-bound
    var placeholder: BoundValue?
    var textFieldType: String?       // shortText/longText/number/date/obscured
    static func parse(_ d: [String: Any]) -> TextFieldProps {
        TextFieldProps(
            label: BoundValue.parse(d["label"]),
            text: BoundValue.parse(d["text"]),
            placeholder: BoundValue.parse(d["placeholder"]),
            textFieldType: d.str("textFieldType")
        )
    }
}

struct CheckboxProps {
    var label: BoundValue?
    var value: BoundValue?           // usually path-bound boolean
    static func parse(_ d: [String: Any]) -> CheckboxProps {
        CheckboxProps(
            label: BoundValue.parse(d["label"]),
            value: BoundValue.parse(d["value"])
        )
    }
}

struct DividerProps {
    var axis: String?                // horizontal/vertical
    static func parse(_ d: [String: Any]) -> DividerProps {
        DividerProps(axis: d.str("axis"))
    }
}

struct CardProps {
    var child: String?
    static func parse(_ d: [String: Any]) -> CardProps {
        CardProps(child: d.str("child"))
    }
}

struct DateTimeInputProps {
    var label: BoundValue?
    var value: BoundValue?           // usually path-bound
    var enableDate: Bool
    var enableTime: Bool
    static func parse(_ d: [String: Any]) -> DateTimeInputProps {
        DateTimeInputProps(
            label: BoundValue.parse(d["label"]),
            value: BoundValue.parse(d["value"]),
            enableDate: d.bool("enableDate") ?? false,
            enableTime: d.bool("enableTime") ?? false
        )
    }
}

// MARK: - Component enum (the catalog “allowlist”)

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
    case unknown(type: String, raw: [String: Any])
}

/// One component instance in the surface component buffer.
struct A2UIComponentInstance {
    let id: String
    let weight: Double?
    let component: A2UIComponent
}

// MARK: - Parsing a v0.8 component wrapper: { "Text": {...} }

enum A2UIStarterCatalogParser {
    static func parseComponentInstance(id: String, weight: Double?, wrapper: [String: Any]) -> A2UIComponentInstance {
        // Wrapper must have exactly one key (component type).
        guard let (type, payloadAny) = wrapper.first else {
            return A2UIComponentInstance(id: id, weight: weight, component: .unknown(type: "InvalidWrapper", raw: wrapper))
        }
        let payload = payloadAny as? [String: Any] ?? [:]

        let component: A2UIComponent
        switch type {
        case "Text":
            if let props = TextProps.parse(payload) { component = .text(props) }
            else { component = .unknown(type: type, raw: payload) }

        case "Column":
            component = .column(ColumnProps.parse(payload))

        case "Row":
            component = .row(RowProps.parse(payload))

        case "Button":
            if let props = ButtonProps.parse(payload) { component = .button(props) }
            else { component = .unknown(type: type, raw: payload) }

        case "TextField":
            component = .textField(TextFieldProps.parse(payload))

        case "Checkbox":
            component = .checkbox(CheckboxProps.parse(payload))

        case "Divider":
            component = .divider(DividerProps.parse(payload))

        case "Card":
            component = .card(CardProps.parse(payload))

        case "DateTimeInput":
            component = .dateTimeInput(DateTimeInputProps.parse(payload))

        default:
            component = .unknown(type: type, raw: payload)
        }

        return A2UIComponentInstance(id: id, weight: weight, component: component)
    }
}

// MARK: - Rendering contract your runtime/store should satisfy

/// Your A2UI runtime can conform to this so the starter catalog can render + write back input state + emit actions.
protocol A2UIStore: ObservableObject {
    // Component lookup
    func component(surfaceId: String, id: String) -> A2UIComponentInstance?

    // BoundValue resolution (literal or look up by path in your data model)
    func resolve(_ bound: BoundValue, surfaceId: String) -> Any?

    // Two-way binding writeback for input components (write to your local data model store).
    func setValue(_ value: Any, at path: String, surfaceId: String)

    // Emit the resolved user action to your agent/network layer.
    func emitUserAction(name: String, surfaceId: String, sourceComponentId: String, context: [String: Any])
}

// MARK: - SwiftUI view that renders any component ID using this starter catalog

struct A2UIComponentView<Store: A2UIStore>: View {
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
            Button {
                guard let action = props.action else { return }
                let ctx = resolveActionContext(action.context)
                store.emitUserAction(
                    name: action.name,
                    surfaceId: surfaceId,
                    sourceComponentId: instance.id,
                    context: ctx
                )
            } label: {
                A2UIComponentView(store: store, surfaceId: surfaceId, componentId: props.child)
            }
            .buttonStyle(props.primary ? .borderedProminent : .bordered)

        case .textField(let props):
            let labelText = (props.label.flatMap { resolvedString($0) }) ?? ""
            let binding = bindingString(props.text, fallbackLiteral: props.text?.literalString)
            if props.textFieldType == "obscured" {
                SecureField(labelText, text: binding)
            } else {
                TextField(labelText, text: binding)
            }

        case .checkbox(let props):
            let labelText = (props.label.flatMap { resolvedString($0) }) ?? ""
            Toggle(labelText, isOn: bindingBool(props.value, fallbackLiteral: props.value?.literalBoolean))

        case .divider:
            Divider()

        case .card(let props):
            GroupBox {
                if let child = props.child {
                    A2UIComponentView(store: store, surfaceId: surfaceId, componentId: child)
                }
            }

        case .dateTimeInput(let props):
            dateTimeInput(props)

        case .unknown(let type, _):
            // Helpful while you expand the catalog.
            Text("Unsupported component: \(type)")
                .font(.footnote)
                .foregroundStyle(.secondary)
        }
    }

    // MARK: - Children rendering

    @ViewBuilder
    private func renderChildren(_ children: ChildrenSpec?) -> some View {
        switch children {
        case .explicitList(let ids):
            ForEach(ids, id: \.self) { id in
                A2UIComponentView(store: store, surfaceId: surfaceId, componentId: id)
            }

        case .template:
            // Template rendering requires per-item instantiation + data binding scoping.
            // Keep this as a clear “next step”.
            Text("Template children not implemented in starter catalog.")
                .font(.footnote)
                .foregroundStyle(.secondary)

        case .none:
            EmptyView()
        }
    }

    // MARK: - BoundValue helpers

    private func resolvedString(_ bv: BoundValue) -> String? {
        if let s = store.resolve(bv, surfaceId: surfaceId) as? String { return s }
        if let n = store.resolve(bv, surfaceId: surfaceId) as? Double { return String(n) }
        if let i = store.resolve(bv, surfaceId: surfaceId) as? Int { return String(i) }
        if let b = store.resolve(bv, surfaceId: surfaceId) as? Bool { return b ? "true" : "false" }
        return bv.literalString
    }

    private func bindingString(_ bv: BoundValue?, fallbackLiteral: String?) -> Binding<String> {
        guard let bv else {
            return Binding(get: { fallbackLiteral ?? "" }, set: { _ in })
        }
        // Prefer two-way binding when path exists.
        if let path = bv.path {
            return Binding(
                get: { (store.resolve(bv, surfaceId: surfaceId) as? String) ?? fallbackLiteral ?? "" },
                set: { newValue in store.setValue(newValue, at: path, surfaceId: surfaceId) }
            )
        }
        // Literal-only => read-only
        return Binding(get: { bv.literalString ?? fallbackLiteral ?? "" }, set: { _ in })
    }

    private func bindingBool(_ bv: BoundValue?, fallbackLiteral: Bool?) -> Binding<Bool> {
        guard let bv else {
            return Binding(get: { fallbackLiteral ?? false }, set: { _ in })
        }
        if let path = bv.path {
            return Binding(
                get: { (store.resolve(bv, surfaceId: surfaceId) as? Bool) ?? fallbackLiteral ?? false },
                set: { newValue in store.setValue(newValue, at: path, surfaceId: surfaceId) }
            )
        }
        return Binding(get: { bv.literalBoolean ?? fallbackLiteral ?? false }, set: { _ in })
    }

    // MARK: - Action resolution

    private func resolveActionContext(_ entries: [ActionContextEntry]) -> [String: Any] {
        var out: [String: Any] = [:]
        for e in entries {
            // If missing, include NSNull so the server sees the key.
            out[e.key] = store.resolve(e.value, surfaceId: surfaceId) ?? NSNull()
        }
        return out
    }

    // MARK: - DateTimeInput (starter)

    @ViewBuilder
    private func dateTimeInput(_ props: DateTimeInputProps) -> some View {
        let label = props.label.flatMap { resolvedString($0) } ?? ""

        // If the server didn’t specify either, don’t render a broken control.
        if !props.enableDate && !props.enableTime {
            Text(label.isEmpty ? "Invalid DateTimeInput" : label)
                .font(.footnote)
                .foregroundStyle(.secondary)
            return
        }

        let binding = bindingDate(props.value, enableDate: props.enableDate, enableTime: props.enableTime)

        DatePicker(
            label,
            selection: binding,
            displayedComponents: displayedComponents(enableDate: props.enableDate, enableTime: props.enableTime)
        )
    }

    private func displayedComponents(enableDate: Bool, enableTime: Bool) -> DatePickerComponents {
        switch (enableDate, enableTime) {
        case (true, true):  return [.date, .hourAndMinute]
        case (true, false): return [.date]
        case (false, true): return [.hourAndMinute]
        default:            return [.date]
        }
    }

    private func bindingDate(_ bv: BoundValue?, enableDate: Bool, enableTime: Bool) -> Binding<Date> {
        guard let bv else {
            return .constant(Date())
        }
        guard let path = bv.path else {
            // literal-only: best-effort parse
            let d = parseA2UIDate(from: bv.literalString) ?? Date()
            return Binding(get: { d }, set: { _ in })
        }

        return Binding(
            get: {
                if let s = store.resolve(bv, surfaceId: surfaceId) as? String {
                    return parseA2UIDate(from: s) ?? Date()
                }
                if let d = store.resolve(bv, surfaceId: surfaceId) as? Date {
                    return d
                }
                if let fallback = bv.literalString {
                    return parseA2UIDate(from: fallback) ?? Date()
                }
                return Date()
            },
            set: { newDate in
                store.setValue(encodeA2UIDate(newDate, enableDate: enableDate, enableTime: enableTime), at: path, surfaceId: surfaceId)
            }
        )
    }

    private func parseA2UIDate(from s: String?) -> Date? {
        guard let s else { return nil }

        // date-only: "YYYY-MM-DD"
        if s.count == 10, s[s.index(s.startIndex, offsetBy: 4)] == "-", s[s.index(s.startIndex, offsetBy: 7)] == "-" {
            let parts = s.split(separator: "-")
            guard parts.count == 3,
                  let y = Int(parts[0]),
                  let m = Int(parts[1]),
                  let d = Int(parts[2]) else { return nil }
            return Calendar.current.date(from: DateComponents(year: y, month: m, day: d))
        }

        // time-only: "HH:mm"
        if s.count == 5, s[s.index(s.startIndex, offsetBy: 2)] == ":" {
            let parts = s.split(separator: ":")
            guard parts.count == 2,
                  let hh = Int(parts[0]),
                  let mm = Int(parts[1]) else { return nil }
            let today = Calendar.current.dateComponents([.year, .month, .day], from: Date())
            return Calendar.current.date(from: DateComponents(year: today.year, month: today.month, day: today.day, hour: hh, minute: mm))
        }

        // datetime: ISO 8601 (best-effort)
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

    // MARK: - Alignment mapping (starter: basic)

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

// MARK: - Tiny Text styling helper

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
```

### How to use this with your runtime
1. **When you parse `surfaceUpdate`**, turn each `{ id, weight?, component }` into an `A2UIComponentInstance` using `A2UIStarterCatalogParser.parseComponentInstance(...)`, then store it in your surface’s component map.
2. **Make your runtime conform to `A2UIStore`** by implementing:
   - component lookup (`component(surfaceId:id:)`)
   - bound resolution (`resolve(_:surfaceId:)`) using your data model + JSON Pointer paths
   - local writeback (`setValue(_:at:surfaceId:)`) for two-way input state
   - action emission (`emitUserAction(...)`) that sends the resolved payload upstream

This matches the v0.8 rules about BoundValue binding/initialization and action context resolution.  [oai_citation:1‡A2UI](https://a2ui.org/specification/v0.8-a2ui/)
