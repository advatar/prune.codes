//
//  ContentView.swift
//  PruneApp
//
//  Created by Johan Sellström on 2026-01-17.
//

import AppKit
import Combine
import SwiftUI
import UniformTypeIdentifiers
import A2UIRuntime

#if canImport(FoundationModels)
import FoundationModels
#endif

struct MenuBarLabel: View {
    let status: AppStatus

    private var tintColor: Color {
        switch status {
        case .running:
            return .green
        case .starting:
            return .orange
        case .stopping:
            return .orange
        case .error:
            return .red
        case .stopped:
            return .gray
        }
    }

    var body: some View {
        HStack(spacing: 6) {
            Image(systemName: status.symbolName)
                .foregroundStyle(tintColor)
            Text("Prune")
        }
    }
}

struct MenuBarView: View {
    @EnvironmentObject private var appModel: AppModel
    @EnvironmentObject private var a2uiAgent: A2UIAgent
    @StateObject private var store = NormalizedSurfaceStore()
    private let surfaceId = "prune_menu"

    private func openSettings(tab: SettingsTab) {
        Task { @MainActor in
            await openSettingsOnMain(tab: tab)
        }
    }

    @MainActor
    private func openSettingsOnMain(tab: SettingsTab) async {
        appModel.selectedTab = tab
        NSApp.activate(ignoringOtherApps: true)
        NSApp.unhide(nil)
        AppLog.info("Open settings requested: tab=\(tab)")
        let didShowSettings = NSApp.sendAction(Selector(("showSettingsWindow:")), to: nil, from: nil)
        if !didShowSettings {
            _ = NSApp.sendAction(Selector(("showPreferencesWindow:")), to: nil, from: nil)
        }
        AppLog.info("showSettingsWindow action result: \(didShowSettings)")
        await bringSettingsWindowToFront()
    }

    @MainActor
    private func bringSettingsWindowToFront() async {
        for _ in 0..<10 {
            NSApp.activate(ignoringOtherApps: true)
            NSApp.unhide(nil)
            if let window = findSettingsWindow() {
                if window.isMiniaturized {
                    window.deminiaturize(nil)
                }
                window.makeKeyAndOrderFront(nil)
                window.orderFrontRegardless()
                NSApp.activate(ignoringOtherApps: true)
                AppLog.info("Settings window focused: \(window.title)")
                return
            }
            await Task.yield()
            try? await Task.sleep(nanoseconds: 80_000_000)
        }
        AppLog.error("Failed to focus settings window after retries.")
    }

    @MainActor
    private func findSettingsWindow() -> NSWindow? {
        if let keyWindow = NSApp.keyWindow, keyWindow.canBecomeKey {
            return keyWindow
        }
        if let mainWindow = NSApp.mainWindow, mainWindow.canBecomeKey {
            return mainWindow
        }
        let candidates = NSApp.orderedWindows.filter { $0.canBecomeKey }
        if let titledWindow = candidates.first(where: { !$0.title.isEmpty }) {
            return titledWindow
        }
        if let visibleWindow = candidates.first(where: { $0.isVisible }) {
            return visibleWindow
        }
        return candidates.first
    }

    var body: some View {
        let bindingProvider = MenuBarBindingProvider(
            appModel: appModel,
            openSettings: openSettings,
            quit: { NSApplication.shared.terminate(nil) }
        )
        A2UISurfaceView(
            store: store,
            surfaceId: surfaceId,
            bindingProvider: bindingProvider,
            style: .menu,
            interactionHandler: { event in
                handleInteraction(event)
            }
        )
        .onAppear {
            a2uiAgent.registerActionHandler(surfaceId: surfaceId) { action, _ in
                bindingProvider.perform(action: action)
            }
            a2uiAgent.render(
                surfaceId: surfaceId,
                store: store,
                template: MenuBarSurface.buildMessages(surfaceId: surfaceId),
                context: menuContext()
            )
        }
        .onDisappear {
            a2uiAgent.removeActionHandler(surfaceId: surfaceId)
        }
    }

    private func menuContext() -> String {
        "status=\(appModel.statusLabel), canStart=\(appModel.canStart), canStop=\(appModel.canStop)"
    }

    private func handleInteraction(_ event: A2UIUserActionEvent) {
        AppLog.info("MenuBar interaction: action=\(event.name) component=\(event.componentId)")
        a2uiAgent.handleUserAction(
            surfaceId: surfaceId,
            store: store,
            template: MenuBarSurface.buildMessages(surfaceId: surfaceId),
            context: menuContext(),
            event: event
        )
    }
}

struct SettingsView: View {
    @EnvironmentObject private var appModel: AppModel
    @EnvironmentObject private var a2uiAgent: A2UIAgent
    @StateObject private var navStore = NormalizedSurfaceStore()
    private let navSurfaceId = "prune_settings_nav"

    var body: some View {
        let bindingProvider = SettingsNavBindingProvider(appModel: appModel)
        VStack(alignment: .leading, spacing: 12) {
            A2UISurfaceView(
                store: navStore,
                surfaceId: navSurfaceId,
                bindingProvider: bindingProvider,
                interactionHandler: { event in
                    handleInteraction(event)
                }
            )
            Divider()
            settingsContent
        }
        .padding(20)
        .frame(minWidth: 760, minHeight: 540)
        .onAppear {
            a2uiAgent.registerActionHandler(surfaceId: navSurfaceId) { action, _ in
                bindingProvider.perform(action: action)
            }
            a2uiAgent.render(
                surfaceId: navSurfaceId,
                store: navStore,
                template: SettingsNavSurface.buildMessages(surfaceId: navSurfaceId),
                context: "selectedTab=\(appModel.selectedTab)"
            )
        }
        .onDisappear {
            a2uiAgent.removeActionHandler(surfaceId: navSurfaceId)
        }
    }

    @ViewBuilder
    private var settingsContent: some View {
        switch appModel.selectedTab {
        case .setup:
            SetupView()
        case .inception:
            InceptionView()
        case .services:
            ServicesView()
        case .integrations:
            IntegrationsView()
        case .a2ui:
            A2UIDiagnosticsView()
        case .help:
            HelpView()
        case .privacy:
            PrivacyView()
        }
    }

    private func handleInteraction(_ event: A2UIUserActionEvent) {
        AppLog.info("Settings nav interaction: action=\(event.name) component=\(event.componentId)")
        a2uiAgent.handleUserAction(
            surfaceId: navSurfaceId,
            store: navStore,
            template: SettingsNavSurface.buildMessages(surfaceId: navSurfaceId),
            context: "selectedTab=\(appModel.selectedTab)",
            event: event
        )
    }
}

struct SetupView: View {
    @EnvironmentObject private var appModel: AppModel
    @EnvironmentObject private var a2uiAgent: A2UIAgent
    @StateObject private var store = NormalizedSurfaceStore()
    private let surfaceId = "prune_setup"

    var body: some View {
        let bindingProvider = SetupBindingProvider(appModel: appModel)
        ScrollView {
            A2UISurfaceView(
                store: store,
                surfaceId: surfaceId,
                bindingProvider: bindingProvider,
                interactionHandler: { event in
                    handleInteraction(event)
                }
            )
            .padding()
        }
        .onAppear {
            a2uiAgent.registerActionHandler(surfaceId: surfaceId) { action, _ in
                bindingProvider.perform(action: action)
            }
            a2uiAgent.render(
                surfaceId: surfaceId,
                store: store,
                template: SetupSurface.buildMessages(surfaceId: surfaceId),
                context: setupContext()
            )
        }
        .onDisappear {
            a2uiAgent.removeActionHandler(surfaceId: surfaceId)
        }
    }

    private func setupContext() -> String {
        "installState=\(appModel.installStateLabel), repo=\(appModel.config.repoFullName)"
    }

    private func handleInteraction(_ event: A2UIUserActionEvent) {
        a2uiAgent.handleUserAction(
            surfaceId: surfaceId,
            store: store,
            template: SetupSurface.buildMessages(surfaceId: surfaceId),
            context: setupContext(),
            event: event
        )
    }
}

@MainActor
private final class SetupBindingProvider: A2UIBindingProvider {
    private let appModel: AppModel

    init(appModel: AppModel) {
        self.appModel = appModel
    }

    func stringValue(for key: String) -> String? {
        switch key {
        case "status.installLabel":
            return appModel.installStateLabel
        case "status.installTone":
            return toneString(appModel.installStateTone)
        case "status.webhookStatusLine":
            return "Webhook Status: \(appModel.webhookStatusLabel)"
        case "paths.appSupport":
            return appModel.paths.appSupport.path
        case "paths.bin":
            return appModel.paths.bin.path
        case "paths.config":
            return appModel.paths.configFile.path
        case "paths.syncStatus":
            return appModel.paths.syncStatusFile.path
        case "paths.logs":
            return appModel.paths.logs.path
        case "paths.launchAgents":
            return appModel.paths.launchAgents.path
        case "status.message":
            return appModel.statusMessage ?? ""
        case "status.error":
            return appModel.lastErrorMessage ?? ""
        default:
            return nil
        }
    }

    func boolValue(for key: String) -> Bool? {
        switch key {
        case "status.installing":
            return appModel.installState == .installing
        default:
            return nil
        }
    }

    func stringBinding(for key: String) -> Binding<String>? {
        switch key {
        case "config.repoFullName":
            return appModel.repoBinding()
        case "config.defaultBranch":
            return appModel.binding(\.defaultBranch)
        case "config.lastIndexedSha":
            return appModel.binding(\.lastIndexedSha)
        case "config.binary.tunnel":
            return appModel.binaryBinding(\.tunnel)
        case "config.binary.sync":
            return appModel.binaryBinding(\.sync)
        case "config.binary.mcp":
            return appModel.binaryBinding(\.mcp)
        default:
            return nil
        }
    }

    func intBinding(for key: String) -> Binding<Int>? {
        switch key {
        case "config.mcpPort":
            return appModel.binding(\.mcpPort)
        case "config.webhookPort":
            return appModel.binding(\.webhookPort)
        default:
            return nil
        }
    }

    func perform(action: String) {
        switch action {
        case "install":
            appModel.install()
        case "reinstall":
            appModel.install(reinstallOnly: true)
        case "detect_repo":
            appModel.detectRepoFromMirror()
        case "pick_repo_folder":
            Task { await appModel.pickRepoFolder() }
        case "copy_app_support":
            appModel.copyToClipboard(appModel.paths.appSupport.path)
        case "copy_bin":
            appModel.copyToClipboard(appModel.paths.bin.path)
        case "copy_config":
            appModel.copyToClipboard(appModel.paths.configFile.path)
        case "copy_sync_status":
            appModel.copyToClipboard(appModel.paths.syncStatusFile.path)
        case "copy_logs":
            appModel.copyToClipboard(appModel.paths.logs.path)
        case "copy_launch_agents":
            appModel.copyToClipboard(appModel.paths.launchAgents.path)
        default:
            break
        }
    }
}

@MainActor
private func resetSurface(_ store: NormalizedSurfaceStore, messages: [NormalizedMsg]) {
    store.reset()
    for message in messages {
        store.apply(message)
    }
}

private func binding(_ key: String) -> JSONValue {
    .object(["binding": .string(key)])
}

private func children(_ ids: [String]) -> JSONValue {
    .array(ids.map { .string($0) })
}

private func toneString(_ tone: StatusTone) -> String {
    switch tone {
    case .good:
        return "good"
    case .warning:
        return "warning"
    case .bad:
        return "bad"
    case .neutral:
        return "neutral"
    }
}

private func encodeJSONL(_ messages: [NormalizedMsg]) -> String {
    messages.compactMap { jsonLine(for: $0) }.joined(separator: "\n")
}

private func jsonLine(for message: NormalizedMsg) -> String? {
    guard let obj = jsonObject(for: message),
          JSONSerialization.isValidJSONObject(obj),
          let data = try? JSONSerialization.data(withJSONObject: obj, options: [.sortedKeys]),
          let text = String(data: data, encoding: .utf8) else {
        return nil
    }
    return text
}

private func jsonObject(for message: NormalizedMsg) -> [String: Any]? {
    switch message {
    case .createSurface(let info):
        var payload: [String: Any] = ["surfaceId": info.surfaceId]
        if let catalogId = info.catalogId {
            payload["catalogId"] = catalogId
        }
        if let root = info.rootComponentId {
            payload["rootComponentId"] = root
        }
        return ["createSurface": payload]
    case .updateComponents(let surfaceId, let components):
        let encodedComponents = components.map { component -> [String: Any] in
            var dict: [String: Any] = [
                "id": component.id,
                "component": component.kind
            ]
            for (key, value) in component.props {
                dict[key] = value.toAny()
            }
            return dict
        }
        return [
            "updateComponents": [
                "surfaceId": surfaceId,
                "components": encodedComponents
            ]
        ]
    case .updateDataModel(let surfaceId, let updates):
        let encodedUpdates = updates.map { update -> [String: Any] in
            var dict: [String: Any] = [
                "path": update.path,
                "value": update.value.toAny()
            ]
            if let metadata = update.metadata {
                dict["metadata"] = metadata.toAny()
            }
            return dict
        }
        return [
            "updateDataModel": [
                "surfaceId": surfaceId,
                "updates": encodedUpdates
            ]
        ]
    case .deleteSurface(let surfaceId):
        return ["deleteSurface": ["surfaceId": surfaceId]]
    case .error:
        return nil
    }
}

struct A2UIUserActionEvent {
    let surfaceId: String
    let componentId: String
    let name: String
    let context: [String: JSONValue]
    let timestamp: Date

    init(
        surfaceId: String,
        componentId: String,
        name: String,
        context: [String: JSONValue] = [:],
        timestamp: Date = Date()
    ) {
        self.surfaceId = surfaceId
        self.componentId = componentId
        self.name = name
        self.context = context
        self.timestamp = timestamp
    }

    func jsonLine() -> String? {
        var payload: [String: Any] = [
            "name": name,
            "surfaceId": surfaceId,
            "sourceComponentId": componentId,
            "timestamp": Self.formatter.string(from: timestamp)
        ]
        if !context.isEmpty {
            payload["context"] = context.mapValues { $0.toAny() }
        }
        let envelope: [String: Any] = ["userAction": payload]
        guard JSONSerialization.isValidJSONObject(envelope),
              let data = try? JSONSerialization.data(withJSONObject: envelope, options: [.sortedKeys]),
              let text = String(data: data, encoding: .utf8) else {
            return nil
        }
        return text
    }

    private static let formatter = ISO8601DateFormatter()
}

private struct A2UIActionRequest {
    let name: String
    let payload: JSONValue?
}

private func a2uiPrompt(
    surfaceId: String,
    context: String,
    template: [NormalizedMsg],
    userAction: String?,
    availableActions: [String]
) -> String {
    let jsonl = encodeJSONL(template)
    let actionsLine = availableActions.isEmpty ? "(none)" : availableActions.joined(separator: ", ")
    let userActionSection = userAction.map { "\nUSER_ACTION_JSONL:\n\($0)\n" } ?? ""

    return """
You are a local A2UI UI generator. Output ONLY JSONL lines (one JSON object per line). Do not include Markdown or extra text.
Use A2UI v0.9 messages: createSurface, updateComponents, updateDataModel.
Keep all component IDs, binding keys, and action IDs exactly as in the template. You may reorder lines but must not change the structure.

ACTION_PROTOCOL:
- User interactions arrive as a JSONL line with a "userAction" envelope (see USER_ACTION_JSONL).
- To request app side-effects, emit updateDataModel with path "/app/actions/<unique>" and value {"name":"<actionId>","payload":{...}}.
- Only use actionId values from AVAILABLE_ACTIONS.
- When USER_ACTION_JSONL includes a name in AVAILABLE_ACTIONS, request that action unless the UI should block it.

SurfaceId: \(surfaceId)
Context: \(context)
AVAILABLE_ACTIONS: \(actionsLine)\(userActionSection)
TEMPLATE_JSONL:
\(jsonl)
"""
}

final class A2UIAgent: ObservableObject {
    enum Mode {
        case live
        case preview
    }

    let objectWillChange = ObservableObjectPublisher()
    private let adapter = A2UIProtocolAdapter(enableV09: true, preferredVersion: .v09)
    private let mode: Mode
    private var inflight: [String: Task<Void, Never>] = [:]
    @MainActor private var actionHandlers: [String: (String, JSONValue?) -> Void] = [:]

    init(mode: Mode = .live) {
        self.mode = mode
    }

    @MainActor
    func registerActionHandler(
        surfaceId: String,
        handler: @escaping (String, JSONValue?) -> Void
    ) {
        actionHandlers[surfaceId] = handler
    }

    @MainActor
    func removeActionHandler(surfaceId: String) {
        actionHandlers.removeValue(forKey: surfaceId)
    }

    @MainActor
    func render(
        surfaceId: String,
        store: NormalizedSurfaceStore,
        template: [NormalizedMsg],
        context: String
    ) {
        inflight[surfaceId]?.cancel()

        let availableActions = templateActionIds(from: template, surfaceId: surfaceId)
        let prompt = a2uiPrompt(
            surfaceId: surfaceId,
            context: context,
            template: template,
            userAction: nil,
            availableActions: availableActions
        )
        let isPreview = (mode == .preview)
        if !isPreview {
            resetSurface(store, messages: template)
        }
        let templateData = template.compactMap { msg -> NormalizedMsg? in
            if case let .updateDataModel(id, updates) = msg, id == surfaceId {
                return .updateDataModel(surfaceId: id, updates: updates)
            }
            return nil
        }
        let templateComponents = templateComponentMap(from: template, surfaceId: surfaceId)
        let templateRootId = templateRootComponentId(from: template, surfaceId: surfaceId)

        let task = Task.detached { [weak self] in
            guard let self else { return }
            if isPreview {
                await MainActor.run {
                    resetSurface(store, messages: template)
                }
                return
            }

            let output = await self.generateMessages(
                prompt: prompt,
                surfaceId: surfaceId,
                templateData: templateData,
                templateComponents: templateComponents,
                templateRootId: templateRootId
            )
            await MainActor.run {
                resetSurface(store, messages: output)
            }
        }

        inflight[surfaceId] = task
    }

    @MainActor
    func handleUserAction(
        surfaceId: String,
        store: NormalizedSurfaceStore,
        template: [NormalizedMsg],
        context: String,
        event: A2UIUserActionEvent
    ) {
        inflight[surfaceId]?.cancel()
        let contextKeys = event.context.keys.sorted().joined(separator: ",")
        AppLog.info("A2UI action: surface=\(surfaceId) name=\(event.name) component=\(event.componentId) contextKeys=[\(contextKeys)]")

        let availableActions = templateActionIds(from: template, surfaceId: surfaceId)
        let userActionLine = event.jsonLine()
        let prompt = a2uiPrompt(
            surfaceId: surfaceId,
            context: context,
            template: template,
            userAction: userActionLine,
            availableActions: availableActions
        )
        let isPreview = (mode == .preview)
        let baselineModel = store.surfaces[surfaceId]?.dataModel
        let templateData = template.compactMap { msg -> NormalizedMsg? in
            if case let .updateDataModel(id, updates) = msg, id == surfaceId {
                return .updateDataModel(surfaceId: id, updates: updates)
            }
            return nil
        }
        let templateComponents = templateComponentMap(from: template, surfaceId: surfaceId)
        let templateRootId = templateRootComponentId(from: template, surfaceId: surfaceId)
        let allowedActionSet = Set(availableActions)
        let canDispatchImmediately = event.name != "input.change" && allowedActionSet.contains(event.name)
        let immediatePayload: JSONValue? = event.context.isEmpty ? nil : .object(event.context)

        if canDispatchImmediately, let handler = actionHandlers[surfaceId] {
            handler(event.name, immediatePayload)
        } else if canDispatchImmediately {
            AppLog.error("A2UI immediate action missing handler: surface=\(surfaceId) name=\(event.name)")
        }

        let task = Task.detached { [weak self] in
            guard let self else { return }
            if isPreview {
                await MainActor.run {
                    resetSurface(store, messages: template)
                }
                return
            }

            let output = await self.generateMessages(
                prompt: prompt,
                surfaceId: surfaceId,
                templateData: templateData,
                templateComponents: templateComponents,
                templateRootId: templateRootId
            )
            let mergedOutput = self.applyBaselineModel(
                output,
                surfaceId: surfaceId,
                baselineModel: baselineModel
            )
            let actionRequests = self.extractActionRequests(
                from: mergedOutput,
                surfaceId: surfaceId,
                allowedActions: allowedActionSet
            )
            let resolvedActions = self.fallbackActionRequests(
                existing: actionRequests,
                event: event,
                allowedActions: allowedActionSet
            )
            await MainActor.run {
                resetSurface(store, messages: mergedOutput)
                if let handler = self.actionHandlers[surfaceId] {
                    for request in resolvedActions where !(canDispatchImmediately && request.name == event.name) {
                        handler(request.name, request.payload)
                    }
                } else if !resolvedActions.isEmpty {
                    AppLog.error("A2UI resolved actions missing handler: surface=\(surfaceId) actions=\(resolvedActions.map { $0.name }.joined(separator: ","))")
                }
            }
        }

        inflight[surfaceId] = task
    }

    private func generateMessages(
        prompt: String,
        surfaceId: String,
        templateData: [NormalizedMsg],
        templateComponents: [String: String],
        templateRootId: String?
    ) async -> [NormalizedMsg] {
#if canImport(FoundationModels)
        if #available(macOS 26.0, *) {
            let model = SystemLanguageModel.default
            guard model.isAvailable else {
                return fallbackSurface(surfaceId: surfaceId, reason: "Local model unavailable.")
            }
            let session = LanguageModelSession(model: model)
            do {
                let response = try await session.respond(to: Prompt(prompt))
                let decoded = decodeMessages(
                    from: response.content,
                    surfaceId: surfaceId,
                    templateData: templateData,
                    templateComponents: templateComponents,
                    templateRootId: templateRootId
                )
                return decoded ?? fallbackSurface(surfaceId: surfaceId, reason: "LLM returned invalid A2UI output.")
            } catch {
                return fallbackSurface(surfaceId: surfaceId, reason: "LLM error: \(error.localizedDescription)")
            }
        }
#endif
        return fallbackSurface(surfaceId: surfaceId, reason: "FoundationModels unavailable on this macOS.")
    }

    private func decodeMessages(
        from text: String,
        surfaceId: String,
        templateData: [NormalizedMsg],
        templateComponents: [String: String],
        templateRootId: String?
    ) -> [NormalizedMsg]? {
        let lines = extractJSONLines(from: text)
        guard !lines.isEmpty else { return nil }

        var decoded: [NormalizedMsg] = []
        var hadError = false
        for line in lines {
            let messages = adapter.decode(line: line)
            for message in messages {
                if case .error = message {
                    hadError = true
                } else {
                    decoded.append(message)
                }
            }
        }

        guard !hadError,
              validate(
                messages: decoded,
                surfaceId: surfaceId,
                templateComponents: templateComponents,
                templateRootId: templateRootId
              ) else {
            return nil
        }

        if !templateData.isEmpty,
           !decoded.contains(where: { msg in
               if case let .updateDataModel(id, _) = msg {
                   return id == surfaceId
               }
               return false
           }) {
            decoded.append(contentsOf: templateData)
        }

        return decoded
    }

    private func validate(
        messages: [NormalizedMsg],
        surfaceId: String,
        templateComponents: [String: String],
        templateRootId: String?
    ) -> Bool {
        var hasCreate = false
        var hasComponents = false
        var rootMatches = templateRootId == nil
        var outputComponents: [String: String] = [:]
        for message in messages {
            switch message {
            case .createSurface(let info):
                if info.surfaceId == surfaceId {
                    hasCreate = true
                    if let templateRootId {
                        rootMatches = info.rootComponentId == templateRootId
                    }
                }
            case .updateComponents(let id, let components):
                if id == surfaceId {
                    hasComponents = true
                    for component in components {
                        outputComponents[component.id] = component.kind
                    }
                }
            default:
                break
            }
        }
        guard hasCreate, hasComponents, rootMatches else {
            return false
        }
        if !templateComponents.isEmpty {
            for (id, kind) in templateComponents {
                guard outputComponents[id] == kind else {
                    return false
                }
            }
        }
        return true
    }

    private func extractJSONLines(from text: String) -> [String] {
        let stripped = text
            .replacingOccurrences(of: "```jsonl", with: "")
            .replacingOccurrences(of: "```json", with: "")
            .replacingOccurrences(of: "```", with: "")

        return stripped
            .split(whereSeparator: \.isNewline)
            .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
            .filter { $0.hasPrefix("{") && $0.hasSuffix("}") }
    }

    private func fallbackSurface(surfaceId: String, reason: String) -> [NormalizedMsg] {
        let rootId = "\(surfaceId)_llm_error_root"
        let titleId = "\(surfaceId)_llm_error_title"
        let detailId = "\(surfaceId)_llm_error_detail"

        let components = [
            NormalizedComponent(
                id: rootId,
                type: "Column",
                props: [
                    "children": .array([.string(titleId), .string(detailId)])
                ]
            ),
            NormalizedComponent(id: titleId, type: "Text", props: [
                "text": .string("A2UI LLM unavailable"),
                "style": .string("headline")
            ]),
            NormalizedComponent(id: detailId, type: "Text", props: [
                "text": .string(reason),
                "style": .string("secondary")
            ])
        ]

        let info = NormalizedSurfaceInfo(
            surfaceId: surfaceId,
            catalogId: "prune.llm.error",
            rootComponentId: rootId,
            protocolVersion: .v09
        )

        return [
            .createSurface(info),
            .updateComponents(surfaceId: surfaceId, components: components)
        ]
    }

    private func templateComponentMap(from template: [NormalizedMsg], surfaceId: String) -> [String: String] {
        var components: [String: String] = [:]
        for message in template {
            guard case let .updateComponents(id, items) = message, id == surfaceId else {
                continue
            }
            for component in items {
                components[component.id] = component.kind
            }
        }
        return components
    }

    private func templateActionIds(from template: [NormalizedMsg], surfaceId: String) -> [String] {
        var actions = Set<String>()
        for message in template {
            guard case let .updateComponents(id, items) = message, id == surfaceId else {
                continue
            }
            for component in items {
                if let action = component.props["action"]?.stringValue, !action.isEmpty {
                    actions.insert(action)
                }
            }
        }
        return actions.sorted()
    }

    private func templateRootComponentId(from template: [NormalizedMsg], surfaceId: String) -> String? {
        for message in template {
            guard case let .createSurface(info) = message, info.surfaceId == surfaceId else {
                continue
            }
            return info.rootComponentId
        }
        return nil
    }

    nonisolated private func extractActionRequests(
        from messages: [NormalizedMsg],
        surfaceId: String,
        allowedActions: Set<String>
    ) -> [A2UIActionRequest] {
        var requests: [A2UIActionRequest] = []
        for message in messages {
            guard case let .updateDataModel(id, updates) = message, id == surfaceId else {
                continue
            }
            for update in updates where update.path.hasPrefix("/app/actions/") {
                if case let .object(obj) = update.value,
                   let name = obj["name"]?.stringValue,
                   allowedActions.contains(name) {
                    requests.append(A2UIActionRequest(name: name, payload: obj["payload"]))
                    continue
                }
                if let name = update.value.stringValue,
                   allowedActions.contains(name) {
                    requests.append(A2UIActionRequest(name: name, payload: nil))
                }
            }
        }
        return requests
    }

    nonisolated private func fallbackActionRequests(
        existing: [A2UIActionRequest],
        event: A2UIUserActionEvent,
        allowedActions: Set<String>
    ) -> [A2UIActionRequest] {
        guard existing.isEmpty else { return existing }
        guard allowedActions.contains(event.name) else { return existing }
        let payload: JSONValue? = event.context.isEmpty ? nil : .object(event.context)
        return [A2UIActionRequest(name: event.name, payload: payload)]
    }

    nonisolated private func applyBaselineModel(
        _ messages: [NormalizedMsg],
        surfaceId: String,
        baselineModel: JSONValue?
    ) -> [NormalizedMsg] {
        guard let baselineModel else { return messages }
        let baseline = NormalizedMsg.updateDataModel(
            surfaceId: surfaceId,
            updates: [NormalizedDataUpdate(path: "/", value: baselineModel)]
        )
        return [baseline] + messages
    }
}

private enum SetupSurface {
    static func buildMessages(surfaceId: String) -> [NormalizedMsg] {
        let components: [NormalizedComponent] = [
            NormalizedComponent(
                id: "setup_root",
                type: "Column",
                props: ["children": children([
                    "setup_title",
                    "setup_divider_1",
                    "install_section",
                    "setup_divider_2",
                    "repo_section",
                    "setup_divider_3",
                    "binaries_section",
                    "setup_divider_4",
                    "paths_section",
                    "setup_divider_5",
                    "ports_section",
                    "setup_divider_6",
                    "status_section"
                ])]
            ),

            NormalizedComponent(id: "setup_title", type: "Text", props: [
                "text": .string("Setup"),
                "style": .string("headline")
            ]),
            NormalizedComponent(id: "setup_divider_1", type: "Divider", props: [:]),
            NormalizedComponent(id: "setup_divider_2", type: "Divider", props: [:]),
            NormalizedComponent(id: "setup_divider_3", type: "Divider", props: [:]),
            NormalizedComponent(id: "setup_divider_4", type: "Divider", props: [:]),
            NormalizedComponent(id: "setup_divider_5", type: "Divider", props: [:]),
            NormalizedComponent(id: "setup_divider_6", type: "Divider", props: [:]),

            NormalizedComponent(id: "install_section", type: "Column", props: [
                "children": children([
                    "install_header",
                    "install_status_row",
                    "install_buttons_row"
                ])
            ]),
            NormalizedComponent(id: "install_header", type: "Text", props: [
                "text": .string("Install"),
                "style": .string("subheadline")
            ]),
            NormalizedComponent(id: "install_status_row", type: "Row", props: [
                "children": children([
                    "install_status_label",
                    "install_status_value",
                    "install_status_spacer"
                ])
            ]),
            NormalizedComponent(id: "install_status_label", type: "Text", props: [
                "text": .string("Status"),
                "style": .string("secondary")
            ]),
            NormalizedComponent(id: "install_status_value", type: "Text", props: [
                "text": binding("status.installLabel"),
                "style": .string("tone"),
                "tone": binding("status.installTone")
            ]),
            NormalizedComponent(id: "install_status_spacer", type: "Spacer", props: [:]),
            NormalizedComponent(id: "install_buttons_row", type: "Row", props: [
                "children": children([
                    "install_button",
                    "reinstall_button"
                ])
            ]),
            NormalizedComponent(id: "install_button", type: "Button", props: [
                "label": .string("Install"),
                "action": .string("install"),
                "variant": .string("primary"),
                "disabled": binding("status.installing")
            ]),
            NormalizedComponent(id: "reinstall_button", type: "Button", props: [
                "label": .string("Reinstall Binaries"),
                "action": .string("reinstall"),
                "disabled": binding("status.installing")
            ]),

            NormalizedComponent(id: "repo_section", type: "Column", props: [
                "children": children([
                    "repo_header",
                    "repo_row",
                    "repo_default_branch",
                    "repo_last_sha",
                    "repo_webhook_status"
                ])
            ]),
            NormalizedComponent(id: "repo_header", type: "Text", props: [
                "text": .string("Repository"),
                "style": .string("subheadline")
            ]),
            NormalizedComponent(id: "repo_row", type: "Row", props: [
                "children": children([
                    "repo_full_name",
                    "repo_detect_button",
                    "repo_pick_button"
                ])
            ]),
            NormalizedComponent(id: "repo_full_name", type: "TextField", props: [
                "label": .string("ORG/REPO"),
                "style": .string("rounded"),
                "value": binding("config.repoFullName")
            ]),
            NormalizedComponent(id: "repo_detect_button", type: "Button", props: [
                "label": .string("Detect from Mirror"),
                "action": .string("detect_repo")
            ]),
            NormalizedComponent(id: "repo_pick_button", type: "Button", props: [
                "label": .string("Pick Folder"),
                "action": .string("pick_repo_folder")
            ]),
            NormalizedComponent(id: "repo_default_branch", type: "TextField", props: [
                "label": .string("Default Branch"),
                "style": .string("rounded"),
                "value": binding("config.defaultBranch")
            ]),
            NormalizedComponent(id: "repo_last_sha", type: "TextField", props: [
                "label": .string("Last Indexed SHA"),
                "style": .string("rounded"),
                "value": binding("config.lastIndexedSha")
            ]),
            NormalizedComponent(id: "repo_webhook_status", type: "Text", props: [
                "text": binding("status.webhookStatusLine"),
                "style": .string("secondary")
            ]),

            NormalizedComponent(id: "binaries_section", type: "Column", props: [
                "children": children([
                    "binaries_header",
                    "binary_tunnel",
                    "binary_sync",
                    "binary_mcp"
                ])
            ]),
            NormalizedComponent(id: "binaries_header", type: "Text", props: [
                "text": .string("Binaries"),
                "style": .string("subheadline")
            ]),
            NormalizedComponent(id: "binary_tunnel", type: "TextField", props: [
                "label": .string("Tunnel Binary"),
                "style": .string("rounded"),
                "value": binding("config.binary.tunnel")
            ]),
            NormalizedComponent(id: "binary_sync", type: "TextField", props: [
                "label": .string("Sync Binary"),
                "style": .string("rounded"),
                "value": binding("config.binary.sync")
            ]),
            NormalizedComponent(id: "binary_mcp", type: "TextField", props: [
                "label": .string("MCP Binary"),
                "style": .string("rounded"),
                "value": binding("config.binary.mcp")
            ]),

            NormalizedComponent(id: "paths_section", type: "Column", props: [
                "children": children([
                    "paths_header",
                    "paths_list"
                ])
            ]),
            NormalizedComponent(id: "paths_header", type: "Text", props: [
                "text": .string("Paths"),
                "style": .string("subheadline")
            ]),
            NormalizedComponent(id: "paths_list", type: "Column", props: [
                "children": children([
                    "path_app_support_row",
                    "path_bin_row",
                    "path_config_row",
                    "path_sync_status_row",
                    "path_logs_row",
                    "path_launch_agents_row"
                ])
            ]),
            NormalizedComponent(id: "path_app_support_row", type: "Row", props: [
                "children": children([
                    "path_app_support_label",
                    "path_app_support_value",
                    "path_app_support_spacer",
                    "path_app_support_copy"
                ])
            ]),
            NormalizedComponent(id: "path_app_support_label", type: "Text", props: [
                "text": .string("App Support"),
                "style": .string("secondary")
            ]),
            NormalizedComponent(id: "path_app_support_value", type: "Text", props: [
                "text": binding("paths.appSupport"),
                "style": .string("monospace"),
                "selectable": .bool(true)
            ]),
            NormalizedComponent(id: "path_app_support_spacer", type: "Spacer", props: [:]),
            NormalizedComponent(id: "path_app_support_copy", type: "Button", props: [
                "label": .string("Copy"),
                "action": .string("copy_app_support")
            ]),
            NormalizedComponent(id: "path_bin_row", type: "Row", props: [
                "children": children([
                    "path_bin_label",
                    "path_bin_value",
                    "path_bin_spacer",
                    "path_bin_copy"
                ])
            ]),
            NormalizedComponent(id: "path_bin_label", type: "Text", props: [
                "text": .string("Bin Directory"),
                "style": .string("secondary")
            ]),
            NormalizedComponent(id: "path_bin_value", type: "Text", props: [
                "text": binding("paths.bin"),
                "style": .string("monospace"),
                "selectable": .bool(true)
            ]),
            NormalizedComponent(id: "path_bin_spacer", type: "Spacer", props: [:]),
            NormalizedComponent(id: "path_bin_copy", type: "Button", props: [
                "label": .string("Copy"),
                "action": .string("copy_bin")
            ]),
            NormalizedComponent(id: "path_config_row", type: "Row", props: [
                "children": children([
                    "path_config_label",
                    "path_config_value",
                    "path_config_spacer",
                    "path_config_copy"
                ])
            ]),
            NormalizedComponent(id: "path_config_label", type: "Text", props: [
                "text": .string("Config File"),
                "style": .string("secondary")
            ]),
            NormalizedComponent(id: "path_config_value", type: "Text", props: [
                "text": binding("paths.config"),
                "style": .string("monospace"),
                "selectable": .bool(true)
            ]),
            NormalizedComponent(id: "path_config_spacer", type: "Spacer", props: [:]),
            NormalizedComponent(id: "path_config_copy", type: "Button", props: [
                "label": .string("Copy"),
                "action": .string("copy_config")
            ]),
            NormalizedComponent(id: "path_sync_status_row", type: "Row", props: [
                "children": children([
                    "path_sync_status_label",
                    "path_sync_status_value",
                    "path_sync_status_spacer",
                    "path_sync_status_copy"
                ])
            ]),
            NormalizedComponent(id: "path_sync_status_label", type: "Text", props: [
                "text": .string("Sync Status"),
                "style": .string("secondary")
            ]),
            NormalizedComponent(id: "path_sync_status_value", type: "Text", props: [
                "text": binding("paths.syncStatus"),
                "style": .string("monospace"),
                "selectable": .bool(true)
            ]),
            NormalizedComponent(id: "path_sync_status_spacer", type: "Spacer", props: [:]),
            NormalizedComponent(id: "path_sync_status_copy", type: "Button", props: [
                "label": .string("Copy"),
                "action": .string("copy_sync_status")
            ]),
            NormalizedComponent(id: "path_logs_row", type: "Row", props: [
                "children": children([
                    "path_logs_label",
                    "path_logs_value",
                    "path_logs_spacer",
                    "path_logs_copy"
                ])
            ]),
            NormalizedComponent(id: "path_logs_label", type: "Text", props: [
                "text": .string("Logs"),
                "style": .string("secondary")
            ]),
            NormalizedComponent(id: "path_logs_value", type: "Text", props: [
                "text": binding("paths.logs"),
                "style": .string("monospace"),
                "selectable": .bool(true)
            ]),
            NormalizedComponent(id: "path_logs_spacer", type: "Spacer", props: [:]),
            NormalizedComponent(id: "path_logs_copy", type: "Button", props: [
                "label": .string("Copy"),
                "action": .string("copy_logs")
            ]),
            NormalizedComponent(id: "path_launch_agents_row", type: "Row", props: [
                "children": children([
                    "path_launch_agents_label",
                    "path_launch_agents_value",
                    "path_launch_agents_spacer",
                    "path_launch_agents_copy"
                ])
            ]),
            NormalizedComponent(id: "path_launch_agents_label", type: "Text", props: [
                "text": .string("LaunchAgents"),
                "style": .string("secondary")
            ]),
            NormalizedComponent(id: "path_launch_agents_value", type: "Text", props: [
                "text": binding("paths.launchAgents"),
                "style": .string("monospace"),
                "selectable": .bool(true)
            ]),
            NormalizedComponent(id: "path_launch_agents_spacer", type: "Spacer", props: [:]),
            NormalizedComponent(id: "path_launch_agents_copy", type: "Button", props: [
                "label": .string("Copy"),
                "action": .string("copy_launch_agents")
            ]),

            NormalizedComponent(id: "ports_section", type: "Column", props: [
                "children": children([
                    "ports_header",
                    "ports_row"
                ])
            ]),
            NormalizedComponent(id: "ports_header", type: "Text", props: [
                "text": .string("Ports"),
                "style": .string("subheadline")
            ]),
            NormalizedComponent(id: "ports_row", type: "Row", props: [
                "children": children([
                    "ports_mcp",
                    "ports_webhook"
                ])
            ]),
            NormalizedComponent(id: "ports_mcp", type: "NumberField", props: [
                "label": .string("MCP Port"),
                "style": .string("rounded"),
                "value": binding("config.mcpPort")
            ]),
            NormalizedComponent(id: "ports_webhook", type: "NumberField", props: [
                "label": .string("Webhook Port"),
                "style": .string("rounded"),
                "value": binding("config.webhookPort")
            ]),

            NormalizedComponent(id: "status_section", type: "Column", props: [
                "children": children([
                    "status_message",
                    "status_error"
                ])
            ]),
            NormalizedComponent(id: "status_message", type: "Text", props: [
                "text": binding("status.message"),
                "style": .string("secondary"),
                "hiddenWhenEmpty": .bool(true)
            ]),
            NormalizedComponent(id: "status_error", type: "Text", props: [
                "text": binding("status.error"),
                "style": .string("error"),
                "hiddenWhenEmpty": .bool(true)
            ])
        ]

        let info = NormalizedSurfaceInfo(
            surfaceId: surfaceId,
            catalogId: "prune.setup",
            rootComponentId: "setup_root",
            protocolVersion: .v09
        )

        return [
            .createSurface(info),
            .updateComponents(surfaceId: surfaceId, components: components)
        ]
    }
}

private enum MenuBarSurface {
    static func buildMessages(surfaceId: String) -> [NormalizedMsg] {
        let components: [NormalizedComponent] = [
            NormalizedComponent(
                id: "menu_root",
                type: "Column",
                props: [
                    "children": children([
                        "menu_status",
                        "menu_message",
                        "menu_divider_1",
                        "menu_start",
                        "menu_stop",
                        "menu_divider_2",
                        "menu_open_dashboard",
                        "menu_view_logs",
                        "menu_open_help",
                        "menu_divider_3",
                        "menu_quit"
                    ])
                ]
            ),
            NormalizedComponent(id: "menu_status", type: "Text", props: [
                "text": binding("menu.status")
            ]),
            NormalizedComponent(id: "menu_message", type: "Text", props: [
                "text": binding("menu.message"),
                "style": .string("secondary"),
                "hiddenWhenEmpty": .bool(true)
            ]),
            NormalizedComponent(id: "menu_divider_1", type: "Divider", props: [:]),
            NormalizedComponent(id: "menu_start", type: "Button", props: [
                "label": .string("Start"),
                "action": .string("start"),
                "variant": .string("primary"),
                "disabled": binding("menu.startDisabled")
            ]),
            NormalizedComponent(id: "menu_stop", type: "Button", props: [
                "label": .string("Stop"),
                "action": .string("stop"),
                "disabled": binding("menu.stopDisabled")
            ]),
            NormalizedComponent(id: "menu_divider_2", type: "Divider", props: [:]),
            NormalizedComponent(id: "menu_open_dashboard", type: "Button", props: [
                "label": .string("Open Dashboard"),
                "action": .string("open_dashboard")
            ]),
            NormalizedComponent(id: "menu_view_logs", type: "Button", props: [
                "label": .string("View Logs"),
                "action": .string("view_logs")
            ]),
            NormalizedComponent(id: "menu_open_help", type: "Button", props: [
                "label": .string("Help"),
                "action": .string("open_help")
            ]),
            NormalizedComponent(id: "menu_divider_3", type: "Divider", props: [:]),
            NormalizedComponent(id: "menu_quit", type: "Button", props: [
                "label": .string("Quit"),
                "action": .string("quit")
            ])
        ]

        let info = NormalizedSurfaceInfo(
            surfaceId: surfaceId,
            catalogId: "prune.menu",
            rootComponentId: "menu_root",
            protocolVersion: .v09
        )

        return [
            .createSurface(info),
            .updateComponents(surfaceId: surfaceId, components: components)
        ]
    }
}

@MainActor
private final class MenuBarBindingProvider: A2UIBindingProvider {
    private let appModel: AppModel
    private let openSettings: (SettingsTab) -> Void
    private let quit: () -> Void

    init(
        appModel: AppModel,
        openSettings: @escaping (SettingsTab) -> Void,
        quit: @escaping () -> Void
    ) {
        self.appModel = appModel
        self.openSettings = openSettings
        self.quit = quit
    }

    func stringValue(for key: String) -> String? {
        switch key {
        case "menu.status":
            return "Status: \(appModel.statusLabel)"
        case "menu.message":
            return appModel.statusMessage ?? ""
        default:
            return nil
        }
    }

    func boolValue(for key: String) -> Bool? {
        switch key {
        case "menu.startDisabled":
            return !appModel.canStart
        case "menu.stopDisabled":
            return !appModel.canStop
        default:
            return nil
        }
    }

    func perform(action: String) {
        AppLog.info("MenuBar action: \(action)")
        switch action {
        case "start":
            if appModel.normalizedRepoFullName() == nil {
                appModel.lastErrorMessage = "Set the repo (Org/Repo) in Setup before starting services."
                openSettings(.setup)
                return
            }
            appModel.startServices()
        case "stop":
            appModel.stopServices()
        case "open_dashboard":
            openSettings(.setup)
        case "view_logs":
            appModel.openLogs()
        case "open_help":
            openSettings(.help)
        case "quit":
            quit()
        default:
            break
        }
    }
}

private enum SettingsNavSurface {
    static func buildMessages(surfaceId: String) -> [NormalizedMsg] {
        let components: [NormalizedComponent] = [
            NormalizedComponent(
                id: "nav_root",
                type: "Column",
                props: [
                    "children": children([
                        "nav_title",
                        "nav_row"
                    ])
                ]
            ),
            NormalizedComponent(id: "nav_title", type: "Text", props: [
                "text": .string("Dashboard"),
                "style": .string("headline")
            ]),
            NormalizedComponent(id: "nav_row", type: "Row", props: [
                "children": children([
                    "nav_setup",
                    "nav_inception",
                    "nav_services",
                    "nav_integrations",
                    "nav_a2ui",
                    "nav_help",
                    "nav_privacy"
                ])
            ]),
            NormalizedComponent(id: "nav_setup", type: "Button", props: [
                "label": .string("Setup"),
                "action": .string("nav.setup"),
                "variant": .string("primary"),
                "disabled": binding("nav.isSetup")
            ]),
            NormalizedComponent(id: "nav_inception", type: "Button", props: [
                "label": .string("Inception"),
                "action": .string("nav.inception"),
                "disabled": binding("nav.isInception")
            ]),
            NormalizedComponent(id: "nav_services", type: "Button", props: [
                "label": .string("Services"),
                "action": .string("nav.services"),
                "disabled": binding("nav.isServices")
            ]),
            NormalizedComponent(id: "nav_integrations", type: "Button", props: [
                "label": .string("Integrations"),
                "action": .string("nav.integrations"),
                "disabled": binding("nav.isIntegrations")
            ]),
            NormalizedComponent(id: "nav_a2ui", type: "Button", props: [
                "label": .string("A2UI"),
                "action": .string("nav.a2ui"),
                "disabled": binding("nav.isA2UI")
            ]),
            NormalizedComponent(id: "nav_help", type: "Button", props: [
                "label": .string("Help"),
                "action": .string("nav.help"),
                "disabled": binding("nav.isHelp")
            ]),
            NormalizedComponent(id: "nav_privacy", type: "Button", props: [
                "label": .string("Privacy"),
                "action": .string("nav.privacy"),
                "disabled": binding("nav.isPrivacy")
            ])
        ]

        let info = NormalizedSurfaceInfo(
            surfaceId: surfaceId,
            catalogId: "prune.settings.nav",
            rootComponentId: "nav_root",
            protocolVersion: .v09
        )

        return [
            .createSurface(info),
            .updateComponents(surfaceId: surfaceId, components: components)
        ]
    }
}

@MainActor
private final class SettingsNavBindingProvider: A2UIBindingProvider {
    private let appModel: AppModel

    init(appModel: AppModel) {
        self.appModel = appModel
    }

    func boolValue(for key: String) -> Bool? {
        switch key {
        case "nav.isSetup":
            return appModel.selectedTab == .setup
        case "nav.isInception":
            return appModel.selectedTab == .inception
        case "nav.isServices":
            return appModel.selectedTab == .services
        case "nav.isIntegrations":
            return appModel.selectedTab == .integrations
        case "nav.isA2UI":
            return appModel.selectedTab == .a2ui
        case "nav.isHelp":
            return appModel.selectedTab == .help
        case "nav.isPrivacy":
            return appModel.selectedTab == .privacy
        default:
            return nil
        }
    }

    func perform(action: String) {
        switch action {
        case "nav.setup":
            appModel.selectedTab = .setup
        case "nav.inception":
            appModel.selectedTab = .inception
        case "nav.services":
            appModel.selectedTab = .services
        case "nav.integrations":
            appModel.selectedTab = .integrations
        case "nav.a2ui":
            appModel.selectedTab = .a2ui
        case "nav.help":
            appModel.selectedTab = .help
        case "nav.privacy":
            appModel.selectedTab = .privacy
        default:
            break
        }
    }
}

// MARK: - Inception

private enum InceptionTemplate: String, CaseIterable, Identifiable {
    case web
    case mobile
    case rust

    var id: String { rawValue }

    var title: String {
        switch self {
        case .web: return "Web (React/Vite/Tailwind + Supabase)"
        case .mobile: return "Mobile (Swift 6 + SwiftUI, Xcode multiplatform)"
        case .rust: return "Rust (CLI/service/library)"
        }
    }

    var defaultStack: [String] {
        switch self {
        case .web: return ["react", "vite", "tailwind", "supabase", "supabase_edge_functions"]
        case .mobile: return ["swiftui", "swift6", "xcode_multiplatform"]
        case .rust: return ["rust"]
        }
    }

    var defaultThinSlice: String {
        switch self {
        case .web:
            return "A small UI that proves the full happy-path: a single screen, one data fetch via Supabase, and one mutation (create/update), with minimal styling."
        case .mobile:
            return "A minimal SwiftUI flow with one list + one detail view, one async data source, and a single write action, wired end-to-end."
        case .rust:
            return "A minimal CLI command that exercises the core logic end-to-end, including parsing args, invoking one core action, and printing structured output."
        }
    }
}

private enum InceptionLandingSurface {
    static func buildMessages(surfaceId: String, hasRepo: Bool) -> [NormalizedMsg] {
        var components: [NormalizedComponent] = [
            NormalizedComponent(
                id: "inception_root",
                type: "Column",
                props: [
                    "children": children(hasRepo ? [
                        "inception_title",
                        "workspace_section",
                        "template_section",
                        "inception_error",
                        "inception_output_title",
                        "inception_output"
                    ] : [
                        "inception_title",
                        "inception_no_repo_title",
                        "inception_no_repo_text"
                    ])
                ]
            ),
            NormalizedComponent(id: "inception_title", type: "Text", props: [
                "text": .string("Inception"),
                "style": .string("headline")
            ])
        ]

        if hasRepo {
            components.append(contentsOf: [
                NormalizedComponent(id: "workspace_section", type: "Column", props: [
                    "children": children([
                        "workspace_title",
                        "workspace_repo",
                        "workspace_path",
                        "workspace_buttons"
                    ])
                ]),
                NormalizedComponent(id: "workspace_title", type: "Text", props: [
                    "text": .string("Workspace"),
                    "style": .string("subheadline")
                ]),
                NormalizedComponent(id: "workspace_repo", type: "Text", props: [
                    "text": binding("inception.repoFullName")
                ]),
                NormalizedComponent(id: "workspace_path", type: "Text", props: [
                    "text": binding("inception.mirrorPath"),
                    "style": .string("monospace"),
                    "selectable": .bool(true)
                ]),
                NormalizedComponent(id: "workspace_buttons", type: "Row", props: [
                    "children": children([
                        "workspace_open_mirror",
                        "workspace_start_interview"
                    ])
                ]),
                NormalizedComponent(id: "workspace_open_mirror", type: "Button", props: [
                    "label": .string("Open Mirror"),
                    "action": .string("open_mirror")
                ]),
                NormalizedComponent(id: "workspace_start_interview", type: "Button", props: [
                    "label": .string("Start A2UI Interview"),
                    "action": .string("start_interview"),
                    "variant": .string("primary")
                ]),
                NormalizedComponent(id: "template_section", type: "Column", props: [
                    "children": children([
                        "template_title",
                        "template_select",
                        "template_toggle",
                        "template_note"
                    ])
                ]),
                NormalizedComponent(id: "template_title", type: "Text", props: [
                    "text": .string("Template"),
                    "style": .string("subheadline")
                ]),
                NormalizedComponent(id: "template_select", type: "Select", props: [
                    "label": .string("Project template"),
                    "value": binding("inception.template"),
                    "options": .array(InceptionTemplate.allCases.map { template in
                        .object([
                            "label": .string(template.title),
                            "value": .string(template.rawValue)
                        ])
                    })
                ]),
                NormalizedComponent(id: "template_toggle", type: "Toggle", props: [
                    "label": .string("Treat this as a CLI-style repo"),
                    "value": binding("inception.cliSubtype")
                ]),
                NormalizedComponent(id: "template_note", type: "Text", props: [
                    "text": .string("This selects the default Prune onboarding, golden paths, and strategy kit. You can refine everything in the interview before bootstrapping."),
                    "style": .string("secondary")
                ]),
                NormalizedComponent(id: "inception_error", type: "Text", props: [
                    "text": binding("inception.error"),
                    "style": .string("error"),
                    "hiddenWhenEmpty": .bool(true)
                ]),
                NormalizedComponent(id: "inception_output_title", type: "Text", props: [
                    "text": binding("inception.outputTitle"),
                    "style": .string("subheadline"),
                    "hiddenWhenEmpty": .bool(true)
                ]),
                NormalizedComponent(id: "inception_output", type: "Text", props: [
                    "text": binding("inception.output"),
                    "style": .string("monospace"),
                    "selectable": .bool(true),
                    "hiddenWhenEmpty": .bool(true)
                ])
            ])
        } else {
            components.append(contentsOf: [
                NormalizedComponent(id: "inception_no_repo_title", type: "Text", props: [
                    "text": .string("No workspace configured"),
                    "style": .string("subheadline")
                ]),
                NormalizedComponent(id: "inception_no_repo_text", type: "Text", props: [
                    "text": .string("Set a GitHub repo in Setup first. Prune will create a local mirror and run inception/bootstrap inside that workspace."),
                    "style": .string("secondary")
                ])
            ])
        }

        let info = NormalizedSurfaceInfo(
            surfaceId: surfaceId,
            catalogId: "prune.inception.landing",
            rootComponentId: "inception_root",
            protocolVersion: .v09
        )

        return [
            .createSurface(info),
            .updateComponents(surfaceId: surfaceId, components: components)
        ]
    }
}

@MainActor
private final class InceptionLandingBindingProvider: A2UIBindingProvider {
    private let appModel: AppModel
    private let template: Binding<String>
    private let cliSubtype: Binding<Bool>
    private let mirrorURL: URL?
    private let openMirror: (URL) -> Void
    private let startInterview: () -> Void
    private let lastOutput: () -> String
    private let lastError: () -> String

    init(
        appModel: AppModel,
        template: Binding<String>,
        cliSubtype: Binding<Bool>,
        mirrorURL: URL?,
        openMirror: @escaping (URL) -> Void,
        startInterview: @escaping () -> Void,
        lastOutput: @escaping () -> String,
        lastError: @escaping () -> String
    ) {
        self.appModel = appModel
        self.template = template
        self.cliSubtype = cliSubtype
        self.mirrorURL = mirrorURL
        self.openMirror = openMirror
        self.startInterview = startInterview
        self.lastOutput = lastOutput
        self.lastError = lastError
    }

    func stringValue(for key: String) -> String? {
        switch key {
        case "inception.repoFullName":
            return appModel.normalizedRepoFullName() ?? ""
        case "inception.mirrorPath":
            return mirrorURL?.path ?? ""
        case "inception.error":
            return lastError()
        case "inception.output":
            return lastOutput()
        case "inception.outputTitle":
            return lastOutput().isEmpty ? "" : "Last output"
        default:
            return nil
        }
    }

    func stringBinding(for key: String) -> Binding<String>? {
        switch key {
        case "inception.template":
            return template
        default:
            return nil
        }
    }

    func boolBinding(for key: String) -> Binding<Bool>? {
        switch key {
        case "inception.cliSubtype":
            return cliSubtype
        default:
            return nil
        }
    }

    func perform(action: String) {
        switch action {
        case "open_mirror":
            if let mirrorURL {
                openMirror(mirrorURL)
            }
        case "start_interview":
            startInterview()
        default:
            break
        }
    }
}

struct InceptionView: View {
    @EnvironmentObject private var appModel: AppModel
    @EnvironmentObject private var a2uiAgent: A2UIAgent

    @State private var template: InceptionTemplate = .web
    @State private var cliSubtype: Bool = false
    @State private var showInterview: Bool = false
    @State private var lastOutput: String = ""
    @State private var lastError: String? = nil
    @StateObject private var store = NormalizedSurfaceStore()
    private let surfaceId = "prune_inception_landing"

    var body: some View {
        let repoFullName = appModel.normalizedRepoFullName()
        let mirror = repoFullName.map { appModel.paths.mirrorDirectory(repoFullName: $0) }
        let bindingProvider = InceptionLandingBindingProvider(
            appModel: appModel,
            template: templateBinding,
            cliSubtype: $cliSubtype,
            mirrorURL: mirror,
            openMirror: { url in
                NSWorkspace.shared.open(url)
            },
            startInterview: {
                lastError = nil
                showInterview = true
            },
            lastOutput: { lastOutput },
            lastError: { lastError ?? "" }
        )

        ScrollView {
            A2UISurfaceView(
                store: store,
                surfaceId: surfaceId,
                bindingProvider: bindingProvider,
                interactionHandler: { event in
                    handleInteraction(event, hasRepo: mirror != nil)
                }
            )
            .padding()
        }
        .onAppear {
            a2uiAgent.registerActionHandler(surfaceId: surfaceId) { action, _ in
                bindingProvider.perform(action: action)
            }
            a2uiAgent.render(
                surfaceId: surfaceId,
                store: store,
                template: InceptionLandingSurface.buildMessages(surfaceId: surfaceId, hasRepo: mirror != nil),
                context: inceptionContext(hasRepo: mirror != nil)
            )
        }
        .onChange(of: appModel.config.repoFullName) { _, _ in
            let hasRepo = appModel.normalizedRepoFullName() != nil
            a2uiAgent.render(
                surfaceId: surfaceId,
                store: store,
                template: InceptionLandingSurface.buildMessages(surfaceId: surfaceId, hasRepo: hasRepo),
                context: inceptionContext(hasRepo: hasRepo)
            )
        }
        .onDisappear {
            a2uiAgent.removeActionHandler(surfaceId: surfaceId)
        }
        .sheet(isPresented: $showInterview) {
            if let mirror {
                InceptionInterviewSheet(
                    template: template,
                    cliSubtype: cliSubtype,
                    repoURL: mirror,
                    onOutput: { out in
                        lastOutput = out
                    },
                    onError: { msg in
                        lastError = msg
                    }
                )
                .environmentObject(appModel)
                .frame(minWidth: 720, minHeight: 560)
            } else {
                NoWorkspaceSheet()
                    .frame(minWidth: 480, minHeight: 220)
            }
        }
    }

    private var templateBinding: Binding<String> {
        Binding(
            get: { template.rawValue },
            set: { rawValue in
                if let newTemplate = InceptionTemplate(rawValue: rawValue) {
                    template = newTemplate
                }
            }
        )
    }

    private func inceptionContext(hasRepo: Bool) -> String {
        "hasRepo=\(hasRepo), template=\(template.rawValue), cliSubtype=\(cliSubtype)"
    }

    private func handleInteraction(_ event: A2UIUserActionEvent, hasRepo: Bool) {
        a2uiAgent.handleUserAction(
            surfaceId: surfaceId,
            store: store,
            template: InceptionLandingSurface.buildMessages(surfaceId: surfaceId, hasRepo: hasRepo),
            context: inceptionContext(hasRepo: hasRepo),
            event: event
        )
    }
}

private struct NoWorkspaceSheet: View {
    @EnvironmentObject private var a2uiAgent: A2UIAgent
    @StateObject private var store = NormalizedSurfaceStore()
    private let surfaceId = "prune_inception_no_repo"

    var body: some View {
        ScrollView {
            A2UISurfaceView(store: store, surfaceId: surfaceId)
                .padding()
        }
        .onAppear {
            a2uiAgent.render(
                surfaceId: surfaceId,
                store: store,
                template: InceptionLandingSurface.buildMessages(surfaceId: surfaceId, hasRepo: false),
                context: "hasRepo=false"
            )
        }
    }
}

private struct InceptionInterviewSheet: View {
    @EnvironmentObject private var appModel: AppModel
    @EnvironmentObject private var a2uiAgent: A2UIAgent

    let template: InceptionTemplate
    let cliSubtype: Bool
    let repoURL: URL
    let onOutput: (String) -> Void
    let onError: (String) -> Void

    @Environment(\.dismiss) private var dismiss
    @StateObject private var store = NormalizedSurfaceStore()
    @State private var status: String = ""
    @State private var isBusy: Bool = false

    private let surfaceId = "prune_inception"
    private let followupsContainerId = "followups_container"
    private let overridesContainerId = "overrides_container"

    var body: some View {
        let bindingProvider = InceptionBindingProvider(
            addOverride: { addManualOverride() },
            generateFollowups: { Task { await generateFollowups() } },
            savePreferences: { Task { await savePreferences() } },
            saveAndBootstrap: { Task { await saveAndBootstrap() } },
            closeInterview: { dismiss() },
            statusText: { status },
            isBusy: { isBusy }
        )
        ScrollView {
            A2UISurfaceView(
                store: store,
                surfaceId: surfaceId,
                bindingProvider: bindingProvider,
                interactionHandler: { event in
                    handleInteraction(event)
                }
            )
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding()
        }
        .onAppear {
            a2uiAgent.registerActionHandler(surfaceId: surfaceId) { action, _ in
                bindingProvider.perform(action: action)
            }
            store.reset()
            let msgs = buildInitialMsgs()
            a2uiAgent.render(
                surfaceId: surfaceId,
                store: store,
                template: msgs,
                context: "template=\(template.rawValue), cliSubtype=\(cliSubtype), repo=\(repoURL.path)"
            )
        }
        .onDisappear {
            a2uiAgent.removeActionHandler(surfaceId: surfaceId)
        }
    }

    private func handleInteraction(_ event: A2UIUserActionEvent) {
        a2uiAgent.handleUserAction(
            surfaceId: surfaceId,
            store: store,
            template: buildInitialMsgs(),
            context: "template=\(template.rawValue), cliSubtype=\(cliSubtype), repo=\(repoURL.path)",
            event: event
        )
    }

    private func buildInitialMsgs() -> [NormalizedMsg] {
        let initialModel = defaultPreferencesDataModel(template: template, cliSubtype: cliSubtype)

        let components: [NormalizedComponent] = [
            NormalizedComponent(
                id: "root",
                type: "Column",
                props: [
                    "children": .array([
                        .string("header_row"),
                        .string("header_note"),
                        .string("divider_header"),
                        .string("title"),
                        .string("subtitle"),
                        .string("divider_1"),
                        .string("project_type"),
                        .string("subtype_cli"),
                        .string("thin_slice"),
                        .string("types_strictness"),
                        .string("tests_required"),
                        .string("deps_policy"),
                        .string("workflow_small_prs"),
                        .string("workflow_commit_style"),
                        .string("divider_2"),
                        .string("followups_title"),
                        .string(followupsContainerId),
                        .string("overrides_title"),
                        .string("overrides_subtitle"),
                        .string(overridesContainerId),
                        .string("overrides_add_button"),
                        .string("divider_actions"),
                        .string("actions_row"),
                        .string("status_text")
                    ])
                ]
            ),

            NormalizedComponent(id: "header_row", type: "Row", props: [
                "children": .array([
                    .string("header_title"),
                    .string("header_spacer"),
                    .string("header_close")
                ])
            ]),
            NormalizedComponent(id: "header_title", type: "Text", props: [
                "text": .string("A2UI Inception Interview"),
                "style": .string("headline")
            ]),
            NormalizedComponent(id: "header_spacer", type: "Spacer", props: [:]),
            NormalizedComponent(id: "header_close", type: "Button", props: [
                "label": .string("Close"),
                "action": .string("close_interview")
            ]),
            NormalizedComponent(id: "header_note", type: "Text", props: [
                "text": .string("Edit preferences, optionally generate extra questions using the local Apple Foundation Model, then save + bootstrap."),
                "style": .string("secondary")
            ]),
            NormalizedComponent(id: "divider_header", type: "Divider", props: [:]),

            NormalizedComponent(id: "title", type: "Text", props: ["text": .string("Prune project preferences")]),
            NormalizedComponent(id: "subtitle", type: "Text", props: ["text": .string("These answers are saved into .prune/prune.preferences.json and used by Prune to bootstrap and guide pruning strategies.")]),
            NormalizedComponent(id: "divider_1", type: "Divider", props: [:]),

            NormalizedComponent(
                id: "project_type",
                type: "Select",
                props: [
                    "label": .string("Project type"),
                    "value": .object(["path": .string("/project_type")]),
                    "options": .array([
                        .object(["label": .string("web"), "value": .string("web")]),
                        .object(["label": .string("mobile"), "value": .string("mobile")]),
                        .object(["label": .string("rust"), "value": .string("rust")])
                    ])
                ]
            ),

            NormalizedComponent(
                id: "subtype_cli",
                type: "Toggle",
                props: [
                    "label": .string("CLI subtype"),
                    "value": .object(["path": .string("/answers/workflow/isCliStyle")])
                ]
            ),

            NormalizedComponent(
                id: "thin_slice",
                type: "TextField",
                props: [
                    "label": .string("Thin slice definition"),
                    "multiline": .bool(true),
                    "value": .object(["path": .string("/answers/scope/thinSliceDefinition")])
                ]
            ),

            NormalizedComponent(
                id: "types_strictness",
                type: "Select",
                props: [
                    "label": .string("Type strictness"),
                    "value": .object(["path": .string("/answers/types/strictness")]),
                    "options": .array([
                        .object(["label": .string("strict"), "value": .string("strict")]),
                        .object(["label": .string("balanced"), "value": .string("balanced")]),
                        .object(["label": .string("loose"), "value": .string("loose")])
                    ])
                ]
            ),

            NormalizedComponent(
                id: "tests_required",
                type: "Toggle",
                props: [
                    "label": .string("Require tests for new features"),
                    "value": .object(["path": .string("/answers/testing/requiredForFeatures")])
                ]
            ),

            NormalizedComponent(
                id: "deps_policy",
                type: "Select",
                props: [
                    "label": .string("New dependency policy"),
                    "value": .object(["path": .string("/answers/deps/newDependencyPolicy")]),
                    "options": .array([
                        .object(["label": .string("avoid_unless_clear_win"), "value": .string("avoid_unless_clear_win")]),
                        .object(["label": .string("ok_for_quality"), "value": .string("ok_for_quality")]),
                        .object(["label": .string("prefer_stdlib"), "value": .string("prefer_stdlib")])
                    ])
                ]
            ),

            NormalizedComponent(
                id: "workflow_small_prs",
                type: "Toggle",
                props: [
                    "label": .string("Prefer small PRs"),
                    "value": .object(["path": .string("/answers/workflow/smallPRsPreferred")])
                ]
            ),

            NormalizedComponent(
                id: "workflow_commit_style",
                type: "Select",
                props: [
                    "label": .string("Commit style"),
                    "value": .object(["path": .string("/answers/workflow/commitStyle")]),
                    "options": .array([
                        .object(["label": .string("concise"), "value": .string("concise")]),
                        .object(["label": .string("detailed"), "value": .string("detailed")])
                    ])
                ]
            ),

            NormalizedComponent(id: "divider_2", type: "Divider", props: [:]),
            NormalizedComponent(id: "followups_title", type: "Text", props: ["text": .string("Generated followups")]),
            NormalizedComponent(id: followupsContainerId, type: "Column", props: ["children": .array([])]),

            NormalizedComponent(id: "overrides_title", type: "Text", props: [
                "text": .string("Manual Q/A overrides")
            ]),
            NormalizedComponent(id: "overrides_subtitle", type: "Text", props: [
                "text": .string("Add your own questions and answers. These override any phrasing shown above."),
                "style": .string("secondary")
            ]),
            NormalizedComponent(id: overridesContainerId, type: "Column", props: ["children": .array([])]),
            NormalizedComponent(id: "overrides_add_button", type: "Button", props: [
                "label": .string("Add Q/A override"),
                "action": .string("add_override"),
                "variant": .string("primary")
            ]),
            NormalizedComponent(id: "divider_actions", type: "Divider", props: [:]),
            NormalizedComponent(id: "actions_row", type: "Row", props: [
                "children": .array([
                    .string("action_generate"),
                    .string("actions_spacer"),
                    .string("action_save"),
                    .string("action_save_bootstrap")
                ])
            ]),
            NormalizedComponent(id: "actions_spacer", type: "Spacer", props: [:]),
            NormalizedComponent(id: "action_generate", type: "Button", props: [
                "label": .string("Generate followups"),
                "action": .string("generate_followups")
            ]),
            NormalizedComponent(id: "action_save", type: "Button", props: [
                "label": .string("Save preferences"),
                "action": .string("save_preferences")
            ]),
            NormalizedComponent(id: "action_save_bootstrap", type: "Button", props: [
                "label": .string("Save + bootstrap"),
                "action": .string("save_bootstrap"),
                "variant": .string("primary"),
                "disabled": binding("inception.busy")
            ]),
            NormalizedComponent(id: "status_text", type: "Text", props: [
                "text": binding("inception.status"),
                "style": .string("secondary"),
                "hiddenWhenEmpty": .bool(true)
            ])
        ]

        let info = NormalizedSurfaceInfo(
            surfaceId: surfaceId,
            catalogId: "prune.inception",
            rootComponentId: "root",
            protocolVersion: .v09
        )

        return [
            .createSurface(info),
            .updateComponents(surfaceId: surfaceId, components: components),
            .updateDataModel(surfaceId: surfaceId, updates: [NormalizedDataUpdate(path: "/", value: initialModel)])
        ]
    }

    private func defaultPreferencesDataModel(template: InceptionTemplate, cliSubtype: Bool) -> JSONValue {
        let base: [String: Any] = [
            "version": 1,
            "project_type": template.rawValue,
            "subtype": cliSubtype ? "Cli" : NSNull(),
            "stack": template.defaultStack,
            "answers": [
                "scope": [
                    "thinSliceDefinition": template.defaultThinSlice
                ],
                "types": [
                    "strictness": template == .web ? "strict" : "balanced"
                ],
                "testing": [
                    "requiredForFeatures": true
                ],
                "deps": [
                    "newDependencyPolicy": "avoid_unless_clear_win"
                ],
                "workflow": [
                    "smallPRsPreferred": true,
                    "commitStyle": "concise",
                    "isCliStyle": cliSubtype
                ]
            ]
        ]

        return JSONValue(fromAny: base)
    }

    @MainActor
    private func savePreferences() async {
        do {
            let model = store.surfaces[surfaceId]?.dataModel ?? .object([:])
            let pruneDir = repoURL.appendingPathComponent(".prune", isDirectory: true)
            try FileManager.default.createDirectory(at: pruneDir, withIntermediateDirectories: true)

            let prefsURL = pruneDir.appendingPathComponent("prune.preferences.json")
            let data = try JSONSerialization.data(withJSONObject: model.toAny(), options: [.prettyPrinted, .sortedKeys])
            try data.write(to: prefsURL, options: .atomic)

            status = "Saved: \(prefsURL.path)"
        } catch {
            onError("Failed to save preferences: \(error.localizedDescription)")
        }
    }

    @MainActor
    private func saveAndBootstrap() async {
        isBusy = true
        defer { isBusy = false }

        await savePreferences()

        do {
            var args = [
                "bootstrap",
                "--repo", repoURL.path,
                "--template", template.rawValue
            ]
            if cliSubtype { args += ["--subtype", "cli"] }

            status = "Running: ce \(args.joined(separator: " "))"
            let output = try await appModel.runCe(args)
            onOutput(output)
            status = "Bootstrap complete."
        } catch {
            onError("Bootstrap failed: \(error.localizedDescription)")
        }
    }

    @MainActor
    private func generateFollowups() async {
        let model = store.surfaces[surfaceId]?.dataModel ?? .object([:])

        do {
            status = "Generating followups..."
            let questions = try await proposeFollowups(template: template, current: model)
            if questions.isEmpty {
                status = "No followups generated."
                return
            }

            // Build new components + data model paths under /answers/followups/<id>
            var newComponents: [NormalizedComponent] = []
            var newChildren: [JSONValue] = []

            for q in questions {
                let blockId = "fu_\(q.id)"
                let questionId = "fu_\(q.id)_question"
                let answerId = "fu_\(q.id)_answer"
                let questionPath = "/answers/followups/\(q.id)/question"
                let answerPath = "/answers/followups/\(q.id)/answer"

                newChildren.append(.string(blockId))
                newComponents.append(
                    NormalizedComponent(
                        id: blockId,
                        type: "Column",
                        props: [
                            "children": .array([.string(questionId), .string(answerId)])
                        ]
                    )
                )
                newComponents.append(
                    NormalizedComponent(
                        id: questionId,
                        type: "TextField",
                        props: [
                            "label": .string("Question"),
                            "multiline": .bool(true),
                            "value": .object(["path": .string(questionPath)])
                        ]
                    )
                )
                newComponents.append(
                    NormalizedComponent(
                        id: answerId,
                        type: "TextField",
                        props: [
                            "label": .string("Answer"),
                            "multiline": .bool(true),
                            "value": .object(["path": .string(answerPath)])
                        ]
                    )
                )

                store.setDataModelValue(surfaceId: surfaceId, path: questionPath, value: .string(q.label))
                store.setDataModelValue(surfaceId: surfaceId, path: answerPath, value: .string(""))
            }

            // Update container children
            if let container = store.component(surfaceId: surfaceId, componentId: followupsContainerId) {
                let existing = (container.props["children"]?.arrayValue ?? [])
                var props = container.props
                props["children"] = .array(existing + newChildren)
                newComponents.append(NormalizedComponent(
                    id: container.id,
                    kind: container.kind,
                    props: props,
                    childrenRefs: container.childrenRefs,
                    childRef: container.childRef
                ))
            }

            store.apply(.updateComponents(surfaceId: surfaceId, components: newComponents))
            status = "Added \(questions.count) followups."
        } catch {
            onError("Followup generation failed: \(error.localizedDescription)")
        }
    }

    private struct FollowupQuestion: Codable, Identifiable {
        var id: String
        var label: String
    }

    private func proposeFollowups(template: InceptionTemplate, current: JSONValue) async throws -> [FollowupQuestion] {
#if canImport(FoundationModels)
        if #available(macOS 26.0, *) {
            let model = SystemLanguageModel.default
            guard model.isAvailable else {
                return []
            }
            let session = LanguageModelSession(model: model)

            let jsonString = String(data: try JSONSerialization.data(withJSONObject: current.toAny(), options: [.sortedKeys]), encoding: .utf8) ?? "{}"

            let promptString = """
You are helping configure a software project at inception.

Project template: \(template.rawValue)
Current preferences JSON:
\(jsonString)

Propose up to 5 additional short questions that would improve the quality of future code changes and pruning.

Return STRICT JSON only, with this exact shape:
[
  {"id": "short_snake_case_key", "label": "Question text"}
]
"""

            let response = try await session.respond(to: Prompt(promptString))
            let text = response.content

            guard let arrayText = extractJSONArray(from: text),
                  let data = arrayText.data(using: .utf8) else {
                return []
            }
            return (try? JSONDecoder().decode([FollowupQuestion].self, from: data)) ?? []
        }
#endif
        return []
    }

    private func extractJSONArray(from text: String) -> String? {
        let start = text.firstIndex(of: "[")
        let end = text.lastIndex(of: "]")
        guard let s = start, let e = end, s < e else { return nil }
        return String(text[s...e])
    }

    @MainActor
    private func addManualOverride() {
        let id = UUID().uuidString.replacingOccurrences(of: "-", with: "")
        let blockId = "override_\(id)"
        let questionId = "override_\(id)_question"
        let answerId = "override_\(id)_answer"
        let questionPath = "/answers/overrides/\(id)/question"
        let answerPath = "/answers/overrides/\(id)/answer"

        var newComponents: [NormalizedComponent] = []
        newComponents.append(
            NormalizedComponent(
                id: blockId,
                type: "Column",
                props: [
                    "children": .array([.string(questionId), .string(answerId)])
                ]
            )
        )
        newComponents.append(
            NormalizedComponent(
                id: questionId,
                type: "TextField",
                props: [
                    "label": .string("Question"),
                    "multiline": .bool(true),
                    "value": .object(["path": .string(questionPath)])
                ]
            )
        )
        newComponents.append(
            NormalizedComponent(
                id: answerId,
                type: "TextField",
                props: [
                    "label": .string("Answer"),
                    "multiline": .bool(true),
                    "value": .object(["path": .string(answerPath)])
                ]
            )
        )

        store.setDataModelValue(surfaceId: surfaceId, path: questionPath, value: .string(""))
        store.setDataModelValue(surfaceId: surfaceId, path: answerPath, value: .string(""))

        if let container = store.component(surfaceId: surfaceId, componentId: overridesContainerId) {
            let existing = (container.props["children"]?.arrayValue ?? [])
            var props = container.props
            props["children"] = .array(existing + [.string(blockId)])
            newComponents.append(NormalizedComponent(
                id: container.id,
                kind: container.kind,
                props: props,
                childrenRefs: container.childrenRefs,
                childRef: container.childRef
            ))
        }

        store.apply(.updateComponents(surfaceId: surfaceId, components: newComponents))
    }
}

@MainActor
private final class InceptionBindingProvider: A2UIBindingProvider {
    private let addOverride: () -> Void
    private let generateFollowups: () -> Void
    private let savePreferences: () -> Void
    private let saveAndBootstrap: () -> Void
    private let closeInterview: () -> Void
    private let statusText: () -> String
    private let isBusy: () -> Bool

    init(
        addOverride: @escaping () -> Void,
        generateFollowups: @escaping () -> Void,
        savePreferences: @escaping () -> Void,
        saveAndBootstrap: @escaping () -> Void,
        closeInterview: @escaping () -> Void,
        statusText: @escaping () -> String,
        isBusy: @escaping () -> Bool
    ) {
        self.addOverride = addOverride
        self.generateFollowups = generateFollowups
        self.savePreferences = savePreferences
        self.saveAndBootstrap = saveAndBootstrap
        self.closeInterview = closeInterview
        self.statusText = statusText
        self.isBusy = isBusy
    }

    func stringValue(for key: String) -> String? {
        switch key {
        case "inception.status":
            return statusText()
        default:
            return nil
        }
    }

    func boolValue(for key: String) -> Bool? {
        switch key {
        case "inception.busy":
            return isBusy()
        default:
            return nil
        }
    }

    func perform(action: String) {
        switch action {
        case "add_override":
            addOverride()
        case "generate_followups":
            generateFollowups()
        case "save_preferences":
            savePreferences()
        case "save_bootstrap":
            saveAndBootstrap()
        case "close_interview":
            closeInterview()
        default:
            break
        }
    }
}

/// A tiny SwiftUI renderer for a limited A2UI component catalog used by Prune views.
@MainActor
private protocol A2UIBindingProvider {
    func stringValue(for key: String) -> String?
    func boolValue(for key: String) -> Bool?
    func stringBinding(for key: String) -> Binding<String>?
    func intBinding(for key: String) -> Binding<Int>?
    func boolBinding(for key: String) -> Binding<Bool>?
    func perform(action: String)
}

@MainActor
private extension A2UIBindingProvider {
    func stringValue(for key: String) -> String? { nil }
    func boolValue(for key: String) -> Bool? { nil }
    func stringBinding(for key: String) -> Binding<String>? { nil }
    func intBinding(for key: String) -> Binding<Int>? { nil }
    func boolBinding(for key: String) -> Binding<Bool>? { nil }
    func perform(action: String) {}
}

private enum A2UISurfaceStyle {
    case standard
    case menu
}

@MainActor
private struct A2UISurfaceView: View {
    @ObservedObject var store: NormalizedSurfaceStore
    let surfaceId: String
    let bindingProvider: (any A2UIBindingProvider)?
    let style: A2UISurfaceStyle
    let interactionHandler: ((A2UIUserActionEvent) -> Void)?

    init(
        store: NormalizedSurfaceStore,
        surfaceId: String,
        bindingProvider: (any A2UIBindingProvider)? = nil,
        style: A2UISurfaceStyle = .standard,
        interactionHandler: ((A2UIUserActionEvent) -> Void)? = nil
    ) {
        self.store = store
        self.surfaceId = surfaceId
        self.bindingProvider = bindingProvider
        self.style = style
        self.interactionHandler = interactionHandler
    }

    var body: some View {
        if let root = store.rootComponentId(for: surfaceId) {
            let content: AnyView = shouldUseStarterCatalog
                ? AnyView(A2UIStarterCatalogSurfaceView(
                    store: store,
                    surfaceId: surfaceId,
                    rootComponentId: root,
                    interactionHandler: interactionHandler
                ))
                : render(componentId: root)
            return applySurfaceStyle(content)
        }
        return AnyView(EmptyView())
    }

    private var shouldUseStarterCatalog: Bool {
        guard let surface = store.surfaces[surfaceId] else { return false }
        if surface.info.protocolVersion == .v08 { return true }
        if let catalogId = surface.info.catalogId {
            return !catalogId.hasPrefix("prune.")
        }
        return false
    }

    private func render(componentId: String) -> AnyView {
        guard let component = store.component(surfaceId: surfaceId, componentId: componentId) else {
            return AnyView(EmptyView())
        }

        let resolved = store.resolvedProps(surfaceId: surfaceId, componentId: componentId) ?? [:]
        let rawProps = store.rawProps(surfaceId: surfaceId, componentId: componentId) ?? [:]

        switch component.type {
        case "Column":
            let children = (component.props["children"]?.arrayValue ?? []).compactMap { $0.stringValue }
            return AnyView(
                VStack(alignment: .leading, spacing: columnSpacing) {
                    ForEach(children, id: \.self) { cid in
                        render(componentId: cid)
                    }
                }
            )

        case "Row":
            let children = (component.props["children"]?.arrayValue ?? []).compactMap { $0.stringValue }
            return AnyView(
                HStack(alignment: .top, spacing: rowSpacing) {
                    ForEach(children, id: \.self) { cid in
                        render(componentId: cid)
                    }
                }
            )

        case "Divider":
            if isMenuStyle {
                return AnyView(
                    Divider()
                        .padding(.horizontal, menuItemHorizontalPadding)
                        .padding(.vertical, 4)
                )
            }
            return AnyView(Divider())

        case "Text":
            let text = resolveText(from: rawProps["text"], fallback: resolved["text"]) ?? ""
            let style = resolved["style"]?.stringValue ?? rawProps["style"]?.stringValue
            let tone = resolveText(from: rawProps["tone"], fallback: resolved["tone"])
            let selectable = resolved["selectable"]?.boolValue ?? rawProps["selectable"]?.boolValue ?? false
            let hideWhenEmpty = resolved["hiddenWhenEmpty"]?.boolValue ?? rawProps["hiddenWhenEmpty"]?.boolValue ?? false

            if hideWhenEmpty && text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                return AnyView(EmptyView())
            }

            var view = Text(text)
            if let style {
                switch style {
                case "headline":
                    view = view.font(.headline)
                case "subheadline":
                    view = view.font(.subheadline)
                case "caption":
                    view = view.font(.caption).foregroundStyle(.secondary)
                case "secondary":
                    view = view.foregroundStyle(.secondary)
                case "error":
                    view = view.foregroundStyle(.red)
                case "monospace":
                    view = view.font(.system(.caption, design: .monospaced))
                case "tone":
                    if let tone = tone {
                        view = view.foregroundStyle(colorForTone(tone))
                    }
                default:
                    break
                }
            }
            let rendered: AnyView = selectable ? AnyView(view.textSelection(.enabled)) : AnyView(view)
            if isMenuStyle {
                return menuItem(rendered, verticalPadding: 2)
            }
            return rendered

        case "TextField":
            let label = resolveText(from: rawProps["label"], fallback: resolved["label"]) ?? ""
            let isMultiline = (component.props["multiline"]?.boolValue) ?? false
            let bindingKey = bindingKey(from: rawProps["value"])
            let path = (component.props["value"]?.objectValue?["path"]?.stringValue) ?? ""
            let style = resolved["style"]?.stringValue ?? rawProps["style"]?.stringValue
            let isReadOnly = resolveBool(from: rawProps["readOnly"], fallback: resolved["readOnly"]) ?? false
            let minHeight = resolved["minHeight"]?.numberValue ?? rawProps["minHeight"]?.numberValue

            if let bindingKey, let binding = bindingProvider?.stringBinding(for: bindingKey) {
                let wrapped = wrapStringBinding(binding, componentId: componentId, bindingKey: bindingKey, path: path)
                if isMultiline {
                    return AnyView(
                        VStack(alignment: .leading, spacing: 6) {
                            if !label.isEmpty { Text(label).font(.caption).foregroundStyle(.secondary) }
                            TextEditor(text: wrapped)
                                .font(textFieldFont(for: style))
                                .frame(minHeight: minHeight ?? 72)
                                .overlay(
                                    RoundedRectangle(cornerRadius: 6)
                                        .strokeBorder(Color.secondary.opacity(0.25), lineWidth: 1)
                                )
                                .disabled(isReadOnly)
                        }
                    )
                }
                let field = TextField(label, text: wrapped)
                return applyTextFieldStyle(field, style: style, readOnly: isReadOnly)
            }

            if isMultiline {
                return AnyView(
                    VStack(alignment: .leading, spacing: 6) {
                        if !label.isEmpty { Text(label).font(.caption).foregroundStyle(.secondary) }
                        TextEditor(text: wrapStringBinding(
                            Binding(
                                get: { store.dataModelValue(surfaceId: surfaceId, path: path)?.stringValue ?? "" },
                                set: { store.setDataModelValue(surfaceId: surfaceId, path: path, value: .string($0)) }
                            ),
                            componentId: componentId,
                            bindingKey: nil,
                            path: path
                        ))
                        .font(textFieldFont(for: style))
                        .frame(minHeight: minHeight ?? 72)
                        .overlay(
                            RoundedRectangle(cornerRadius: 6)
                                .strokeBorder(Color.secondary.opacity(0.25), lineWidth: 1)
                        )
                        .disabled(isReadOnly)
                    }
                )
            }
            let field = TextField(label, text: wrapStringBinding(
                Binding(
                    get: { store.dataModelValue(surfaceId: surfaceId, path: path)?.stringValue ?? "" },
                    set: { store.setDataModelValue(surfaceId: surfaceId, path: path, value: .string($0)) }
                ),
                componentId: componentId,
                bindingKey: nil,
                path: path
            ))
            return applyTextFieldStyle(field, style: style, readOnly: isReadOnly)

        case "SecureField":
            let label = resolveText(from: rawProps["label"], fallback: resolved["label"]) ?? ""
            let bindingKey = bindingKey(from: rawProps["value"])
            let path = (component.props["value"]?.objectValue?["path"]?.stringValue) ?? ""
            let style = resolved["style"]?.stringValue ?? rawProps["style"]?.stringValue
            let isReadOnly = resolveBool(from: rawProps["readOnly"], fallback: resolved["readOnly"]) ?? false

            if let bindingKey, let binding = bindingProvider?.stringBinding(for: bindingKey) {
                let wrapped = wrapStringBinding(binding, componentId: componentId, bindingKey: bindingKey, path: path)
                let field = SecureField(label, text: wrapped)
                return applyTextFieldStyle(field, style: style, readOnly: isReadOnly)
            }
            let field = SecureField(label, text: wrapStringBinding(
                Binding(
                    get: { store.dataModelValue(surfaceId: surfaceId, path: path)?.stringValue ?? "" },
                    set: { store.setDataModelValue(surfaceId: surfaceId, path: path, value: .string($0)) }
                ),
                componentId: componentId,
                bindingKey: nil,
                path: path
            ))
            return applyTextFieldStyle(field, style: style, readOnly: isReadOnly)

        case "Toggle":
            let label = resolveText(from: rawProps["label"], fallback: resolved["label"]) ?? ""
            let bindingKey = bindingKey(from: rawProps["value"])
            let path = (component.props["value"]?.objectValue?["path"]?.stringValue) ?? ""
            if let bindingKey, let binding = bindingProvider?.boolBinding(for: bindingKey) {
                let wrapped = wrapBoolBinding(binding, componentId: componentId, bindingKey: bindingKey, path: path)
                return AnyView(Toggle(label, isOn: wrapped))
            }
            return AnyView(
                Toggle(label, isOn: wrapBoolBinding(
                    Binding(
                        get: { store.dataModelValue(surfaceId: surfaceId, path: path)?.boolValue ?? false },
                        set: { store.setDataModelValue(surfaceId: surfaceId, path: path, value: .bool($0)) }
                    ),
                    componentId: componentId,
                    bindingKey: nil,
                    path: path
                ))
            )

        case "Select":
            let label = resolveText(from: rawProps["label"], fallback: resolved["label"]) ?? ""
            let bindingKey = bindingKey(from: rawProps["value"])
            let path = (component.props["value"]?.objectValue?["path"]?.stringValue) ?? ""
            let options = (component.props["options"]?.arrayValue ?? []).compactMap { opt -> (String, String)? in
                if let obj = opt.objectValue {
                    return (obj["label"]?.stringValue ?? "", obj["value"]?.stringValue ?? "")
                }
                if let s = opt.stringValue { return (s, s) }
                return nil
            }

            if let bindingKey, let binding = bindingProvider?.stringBinding(for: bindingKey) {
                let wrapped = wrapStringBinding(binding, componentId: componentId, bindingKey: bindingKey, path: path)
                return AnyView(
                    Picker(label, selection: wrapped) {
                        ForEach(options, id: \.1) { (lbl, val) in
                            Text(lbl).tag(val)
                        }
                    }
                    .pickerStyle(.menu)
                )
            }

            return AnyView(
                Picker(label, selection: wrapStringBinding(
                    Binding(
                        get: { store.dataModelValue(surfaceId: surfaceId, path: path)?.stringValue ?? "" },
                        set: { store.setDataModelValue(surfaceId: surfaceId, path: path, value: .string($0)) }
                    ),
                    componentId: componentId,
                    bindingKey: nil,
                    path: path
                )) {
                    ForEach(options, id: \.1) { (lbl, val) in
                        Text(lbl).tag(val)
                    }
                }
                .pickerStyle(.menu)
            )

        case "NumberField":
            let label = resolveText(from: rawProps["label"], fallback: resolved["label"]) ?? ""
            let bindingKey = bindingKey(from: rawProps["value"])
            let style = resolved["style"]?.stringValue ?? rawProps["style"]?.stringValue
            if let bindingKey, let binding = bindingProvider?.intBinding(for: bindingKey) {
                let wrapped = wrapIntBinding(binding, componentId: componentId, bindingKey: bindingKey, path: nil)
                let field = TextField(label, value: wrapped, format: .number)
                return applyTextFieldStyle(field, style: style, readOnly: false)
            }
            return AnyView(EmptyView())

        case "Button":
            let label = resolveText(from: rawProps["label"], fallback: resolved["label"]) ?? "Action"
            let actionId = resolved["action"]?.stringValue ?? rawProps["action"]?.stringValue ?? ""
            let variant = resolved["variant"]?.stringValue ?? rawProps["variant"]?.stringValue
            let disabled = resolveBool(from: rawProps["disabled"], fallback: resolved["disabled"]) ?? false
            let button = Button(label) {
                guard !actionId.isEmpty else { return }
                sendUserAction(name: actionId, componentId: componentId)
            }
            .disabled(disabled)
            if isMenuStyle {
                return AnyView(
                    button.buttonStyle(MenuBarButtonStyle(isPrimary: variant == "primary"))
                )
            }
            if variant == "primary" {
                return AnyView(button.buttonStyle(.borderedProminent))
            }
            return AnyView(button.buttonStyle(.bordered))

        case "Spacer":
            return AnyView(Spacer())

        default:
            // Unknown component in this minimal renderer.
            let fallback = "[Unsupported component: \(component.type)]"
            return AnyView(
                Text(fallback)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            )
        }
    }

    private func sendUserAction(
        name: String,
        componentId: String,
        context: [String: JSONValue] = [:]
    ) {
        guard let interactionHandler else { return }
        interactionHandler(A2UIUserActionEvent(
            surfaceId: surfaceId,
            componentId: componentId,
            name: name,
            context: context
        ))
    }

    private func inputContext(
        bindingKey: String?,
        path: String?,
        value: JSONValue
    ) -> [String: JSONValue] {
        var context: [String: JSONValue] = ["value": value]
        if let bindingKey {
            context["binding"] = .string(bindingKey)
        }
        if let path, !path.isEmpty {
            context["path"] = .string(path)
        }
        return context
    }

    private func wrapStringBinding(
        _ binding: Binding<String>,
        componentId: String,
        bindingKey: String?,
        path: String?
    ) -> Binding<String> {
        Binding(
            get: { binding.wrappedValue },
            set: { newValue in
                binding.wrappedValue = newValue
                sendUserAction(
                    name: "input.change",
                    componentId: componentId,
                    context: inputContext(bindingKey: bindingKey, path: path, value: .string(newValue))
                )
            }
        )
    }

    private func wrapBoolBinding(
        _ binding: Binding<Bool>,
        componentId: String,
        bindingKey: String?,
        path: String?
    ) -> Binding<Bool> {
        Binding(
            get: { binding.wrappedValue },
            set: { newValue in
                binding.wrappedValue = newValue
                sendUserAction(
                    name: "input.change",
                    componentId: componentId,
                    context: inputContext(bindingKey: bindingKey, path: path, value: .bool(newValue))
                )
            }
        )
    }

    private func wrapIntBinding(
        _ binding: Binding<Int>,
        componentId: String,
        bindingKey: String?,
        path: String?
    ) -> Binding<Int> {
        Binding(
            get: { binding.wrappedValue },
            set: { newValue in
                binding.wrappedValue = newValue
                sendUserAction(
                    name: "input.change",
                    componentId: componentId,
                    context: inputContext(bindingKey: bindingKey, path: path, value: .number(Double(newValue)))
                )
            }
        )
    }

    private func bindingKey(from value: JSONValue?) -> String? {
        guard case let .object(obj) = value,
              let binding = obj["binding"]?.stringValue else {
            return nil
        }
        return binding
    }

    private func resolveText(from raw: JSONValue?, fallback: JSONValue?) -> String? {
        if let key = bindingKey(from: raw) {
            return bindingProvider?.stringValue(for: key)
        }
        return fallback?.stringValue ?? raw?.stringValue
    }

    private func resolveBool(from raw: JSONValue?, fallback: JSONValue?) -> Bool? {
        if let key = bindingKey(from: raw) {
            return bindingProvider?.boolValue(for: key)
        }
        return fallback?.boolValue ?? raw?.boolValue
    }

    private func colorForTone(_ tone: String) -> Color {
        switch tone {
        case "good":
            return .green
        case "warning":
            return .orange
        case "bad":
            return .red
        default:
            return .gray
        }
    }

    private func textFieldFont(for style: String?) -> Font? {
        if style == "monospace" {
            return .system(.body, design: .monospaced)
        }
        return nil
    }

    private func applyTextFieldStyle<V: View>(_ view: V, style: String?, readOnly: Bool) -> AnyView {
        var result: AnyView = AnyView(view)
        if let font = textFieldFont(for: style) {
            result = AnyView(result.font(font))
        }
        if style == "rounded" {
            result = AnyView(result.textFieldStyle(.roundedBorder))
        }
        if readOnly {
            result = AnyView(result.disabled(true))
        }
        return result
    }

    private var isMenuStyle: Bool {
        style == .menu
    }

    private var columnSpacing: CGFloat {
        isMenuStyle ? 6 : 12
    }

    private var rowSpacing: CGFloat {
        isMenuStyle ? 8 : 12
    }

    private var menuItemHorizontalPadding: CGFloat {
        10
    }

    private func applySurfaceStyle(_ view: AnyView) -> AnyView {
        guard isMenuStyle else { return view }
        return AnyView(
            view
                .padding(.vertical, 8)
                .frame(minWidth: 240, alignment: .leading)
        )
    }

    private func menuItem(_ view: AnyView, verticalPadding: CGFloat) -> AnyView {
        AnyView(
            view
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.horizontal, menuItemHorizontalPadding)
                .padding(.vertical, verticalPadding)
        )
    }

    private struct MenuBarButtonStyle: ButtonStyle {
        let isPrimary: Bool

        func makeBody(configuration: Configuration) -> some View {
            configuration.label
                .font(.system(size: 13, weight: isPrimary ? .semibold : .regular))
                .foregroundStyle(isPrimary ? Color.white : Color.primary)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.vertical, 6)
                .padding(.horizontal, 10)
                .background(
                    RoundedRectangle(cornerRadius: 8)
                        .fill(backgroundColor(pressed: configuration.isPressed))
                )
        }

        private func backgroundColor(pressed: Bool) -> Color {
            if isPrimary {
                return Color.accentColor.opacity(pressed ? 0.65 : 0.55)
            }
            return Color.primary.opacity(pressed ? 0.18 : 0.1)
        }
    }
}

private enum ServicesSurface {
    static func buildMessages(surfaceId: String) -> [NormalizedMsg] {
        let components: [NormalizedComponent] = [
            NormalizedComponent(
                id: "services_root",
                type: "Column",
                props: [
                    "children": children([
                        "services_title",
                        "services_buttons",
                        "services_launchagents",
                        "services_list",
                        "services_args",
                        "services_logs",
                        "services_status"
                    ])
                ]
            ),
            NormalizedComponent(id: "services_title", type: "Text", props: [
                "text": .string("Services"),
                "style": .string("headline")
            ]),
            NormalizedComponent(id: "services_buttons", type: "Row", props: [
                "children": children([
                    "services_start",
                    "services_stop",
                    "services_refresh"
                ])
            ]),
            NormalizedComponent(id: "services_start", type: "Button", props: [
                "label": .string("Start"),
                "action": .string("services.start"),
                "variant": .string("primary"),
                "disabled": binding("services.startDisabled")
            ]),
            NormalizedComponent(id: "services_stop", type: "Button", props: [
                "label": .string("Stop"),
                "action": .string("services.stop"),
                "disabled": binding("services.stopDisabled")
            ]),
            NormalizedComponent(id: "services_refresh", type: "Button", props: [
                "label": .string("Refresh Status"),
                "action": .string("services.refresh"),
                "disabled": binding("services.refreshDisabled")
            ]),
            NormalizedComponent(id: "services_launchagents", type: "Column", props: [
                "children": children([
                    "services_launchagents_title",
                    "services_launchagents_toggle",
                    "services_launchagents_buttons"
                ])
            ]),
            NormalizedComponent(id: "services_launchagents_title", type: "Text", props: [
                "text": .string("LaunchAgents"),
                "style": .string("subheadline")
            ]),
            NormalizedComponent(id: "services_launchagents_toggle", type: "Toggle", props: [
                "label": .string("Manage services with LaunchAgents"),
                "value": binding("services.useLaunchAgents")
            ]),
            NormalizedComponent(id: "services_launchagents_buttons", type: "Row", props: [
                "children": children([
                    "services_launchagents_install",
                    "services_launchagents_remove",
                    "services_launchagents_open"
                ])
            ]),
            NormalizedComponent(id: "services_launchagents_install", type: "Button", props: [
                "label": .string("Install LaunchAgents"),
                "action": .string("services.installLaunchAgents")
            ]),
            NormalizedComponent(id: "services_launchagents_remove", type: "Button", props: [
                "label": .string("Remove LaunchAgents"),
                "action": .string("services.removeLaunchAgents")
            ]),
            NormalizedComponent(id: "services_launchagents_open", type: "Button", props: [
                "label": .string("Open LaunchAgents Folder"),
                "action": .string("services.openLaunchAgentsFolder")
            ]),
            NormalizedComponent(id: "services_list", type: "Column", props: [
                "children": children([
                    "service_tunnel_row",
                    "service_tunnel_detail",
                    "service_sync_row",
                    "service_sync_detail",
                    "service_mcp_row",
                    "service_mcp_detail"
                ])
            ]),
            NormalizedComponent(id: "service_tunnel_row", type: "Row", props: [
                "children": children([
                    "service_tunnel_name",
                    "service_tunnel_spacer",
                    "service_tunnel_status"
                ])
            ]),
            NormalizedComponent(id: "service_tunnel_name", type: "Text", props: [
                "text": .string("Tunnel"),
                "style": .string("subheadline")
            ]),
            NormalizedComponent(id: "service_tunnel_spacer", type: "Spacer", props: [:]),
            NormalizedComponent(id: "service_tunnel_status", type: "Text", props: [
                "text": binding("services.tunnel.status"),
                "style": .string("tone"),
                "tone": binding("services.tunnel.tone")
            ]),
            NormalizedComponent(id: "service_tunnel_detail", type: "Text", props: [
                "text": binding("services.tunnel.detail"),
                "style": .string("secondary"),
                "hiddenWhenEmpty": .bool(true)
            ]),
            NormalizedComponent(id: "service_sync_row", type: "Row", props: [
                "children": children([
                    "service_sync_name",
                    "service_sync_spacer",
                    "service_sync_status"
                ])
            ]),
            NormalizedComponent(id: "service_sync_name", type: "Text", props: [
                "text": .string("Sync"),
                "style": .string("subheadline")
            ]),
            NormalizedComponent(id: "service_sync_spacer", type: "Spacer", props: [:]),
            NormalizedComponent(id: "service_sync_status", type: "Text", props: [
                "text": binding("services.sync.status"),
                "style": .string("tone"),
                "tone": binding("services.sync.tone")
            ]),
            NormalizedComponent(id: "service_sync_detail", type: "Text", props: [
                "text": binding("services.sync.detail"),
                "style": .string("secondary"),
                "hiddenWhenEmpty": .bool(true)
            ]),
            NormalizedComponent(id: "service_mcp_row", type: "Row", props: [
                "children": children([
                    "service_mcp_name",
                    "service_mcp_spacer",
                    "service_mcp_status"
                ])
            ]),
            NormalizedComponent(id: "service_mcp_name", type: "Text", props: [
                "text": .string("MCP"),
                "style": .string("subheadline")
            ]),
            NormalizedComponent(id: "service_mcp_spacer", type: "Spacer", props: [:]),
            NormalizedComponent(id: "service_mcp_status", type: "Text", props: [
                "text": binding("services.mcp.status"),
                "style": .string("tone"),
                "tone": binding("services.mcp.tone")
            ]),
            NormalizedComponent(id: "service_mcp_detail", type: "Text", props: [
                "text": binding("services.mcp.detail"),
                "style": .string("secondary"),
                "hiddenWhenEmpty": .bool(true)
            ]),
            NormalizedComponent(id: "services_args", type: "Column", props: [
                "children": children([
                    "services_args_title",
                    "services_args_tunnel",
                    "services_args_sync",
                    "services_args_mcp",
                    "services_args_note"
                ])
            ]),
            NormalizedComponent(id: "services_args_title", type: "Text", props: [
                "text": .string("Service Arguments"),
                "style": .string("subheadline")
            ]),
            NormalizedComponent(id: "services_args_tunnel", type: "TextField", props: [
                "label": .string("Tunnel"),
                "multiline": .bool(true),
                "style": .string("monospace"),
                "minHeight": .number(70),
                "value": binding("services.args.tunnel")
            ]),
            NormalizedComponent(id: "services_args_sync", type: "TextField", props: [
                "label": .string("Sync"),
                "multiline": .bool(true),
                "style": .string("monospace"),
                "minHeight": .number(70),
                "value": binding("services.args.sync")
            ]),
            NormalizedComponent(id: "services_args_mcp", type: "TextField", props: [
                "label": .string("MCP"),
                "multiline": .bool(true),
                "style": .string("monospace"),
                "minHeight": .number(70),
                "value": binding("services.args.mcp")
            ]),
            NormalizedComponent(id: "services_args_note", type: "Text", props: [
                "text": .string("Arguments are optional and appended to defaults. One argument per line."),
                "style": .string("secondary")
            ]),
            NormalizedComponent(id: "services_logs", type: "Column", props: [
                "children": children([
                    "services_logs_title",
                    "services_logs_buttons",
                    "services_logs_preview"
                ])
            ]),
            NormalizedComponent(id: "services_logs_title", type: "Text", props: [
                "text": .string("Logs"),
                "style": .string("subheadline")
            ]),
            NormalizedComponent(id: "services_logs_buttons", type: "Row", props: [
                "children": children([
                    "services_logs_view",
                    "services_logs_refresh"
                ])
            ]),
            NormalizedComponent(id: "services_logs_view", type: "Button", props: [
                "label": .string("View Logs"),
                "action": .string("services.viewLogs")
            ]),
            NormalizedComponent(id: "services_logs_refresh", type: "Button", props: [
                "label": .string("Refresh Logs"),
                "action": .string("services.refreshLogs")
            ]),
            NormalizedComponent(id: "services_logs_preview", type: "TextField", props: [
                "label": .string("Log Preview"),
                "multiline": .bool(true),
                "style": .string("monospace"),
                "minHeight": .number(180),
                "readOnly": .bool(true),
                "value": binding("services.logPreview")
            ]),
            NormalizedComponent(id: "services_status", type: "Column", props: [
                "children": children([
                    "services_status_message",
                    "services_status_error"
                ])
            ]),
            NormalizedComponent(id: "services_status_message", type: "Text", props: [
                "text": binding("status.message"),
                "style": .string("secondary"),
                "hiddenWhenEmpty": .bool(true)
            ]),
            NormalizedComponent(id: "services_status_error", type: "Text", props: [
                "text": binding("status.error"),
                "style": .string("error"),
                "hiddenWhenEmpty": .bool(true)
            ])
        ]

        let info = NormalizedSurfaceInfo(
            surfaceId: surfaceId,
            catalogId: "prune.services",
            rootComponentId: "services_root",
            protocolVersion: .v09
        )

        return [
            .createSurface(info),
            .updateComponents(surfaceId: surfaceId, components: components)
        ]
    }
}

@MainActor
private final class ServicesBindingProvider: A2UIBindingProvider {
    private let appModel: AppModel

    init(appModel: AppModel) {
        self.appModel = appModel
    }

    func stringValue(for key: String) -> String? {
        switch key {
        case "services.tunnel.status":
            return appModel.serviceStatus(for: .tunnel).state.label
        case "services.tunnel.tone":
            return toneString(appModel.serviceStatus(for: .tunnel).state.tone)
        case "services.tunnel.detail":
            return appModel.serviceStatus(for: .tunnel).detail
        case "services.sync.status":
            return appModel.serviceStatus(for: .sync).state.label
        case "services.sync.tone":
            return toneString(appModel.serviceStatus(for: .sync).state.tone)
        case "services.sync.detail":
            return appModel.serviceStatus(for: .sync).detail
        case "services.mcp.status":
            return appModel.serviceStatus(for: .mcp).state.label
        case "services.mcp.tone":
            return toneString(appModel.serviceStatus(for: .mcp).state.tone)
        case "services.mcp.detail":
            return appModel.serviceStatus(for: .mcp).detail
        case "services.logPreview":
            return appModel.logPreview
        case "status.message":
            return appModel.statusMessage ?? ""
        case "status.error":
            return appModel.lastErrorMessage ?? ""
        default:
            return nil
        }
    }

    func boolValue(for key: String) -> Bool? {
        switch key {
        case "services.startDisabled":
            return !appModel.canStart
        case "services.stopDisabled":
            return !appModel.canStop
        case "services.refreshDisabled":
            return appModel.installState != .installed
        default:
            return nil
        }
    }

    func boolBinding(for key: String) -> Binding<Bool>? {
        switch key {
        case "services.useLaunchAgents":
            return appModel.binding(\.useLaunchAgents)
        default:
            return nil
        }
    }

    func stringBinding(for key: String) -> Binding<String>? {
        switch key {
        case "services.args.tunnel":
            return appModel.argumentsBinding(for: .tunnel)
        case "services.args.sync":
            return appModel.argumentsBinding(for: .sync)
        case "services.args.mcp":
            return appModel.argumentsBinding(for: .mcp)
        default:
            return nil
        }
    }

    func perform(action: String) {
        switch action {
        case "services.start":
            appModel.startServices()
        case "services.stop":
            appModel.stopServices()
        case "services.refresh":
            appModel.refreshStatus()
        case "services.installLaunchAgents":
            appModel.installLaunchAgents()
        case "services.removeLaunchAgents":
            appModel.removeLaunchAgents()
        case "services.openLaunchAgentsFolder":
            appModel.openLaunchAgentsFolder()
        case "services.viewLogs":
            appModel.openLogs()
        case "services.refreshLogs":
            appModel.refreshLogPreview()
        default:
            break
        }
    }
}

struct ServicesView: View {
    @EnvironmentObject private var appModel: AppModel
    @EnvironmentObject private var a2uiAgent: A2UIAgent
    @StateObject private var store = NormalizedSurfaceStore()
    private let surfaceId = "prune_services"

    var body: some View {
        let bindingProvider = ServicesBindingProvider(appModel: appModel)
        ScrollView {
            A2UISurfaceView(
                store: store,
                surfaceId: surfaceId,
                bindingProvider: bindingProvider,
                interactionHandler: { event in
                    handleInteraction(event)
                }
            )
            .padding()
        }
        .onAppear {
            a2uiAgent.registerActionHandler(surfaceId: surfaceId) { action, _ in
                bindingProvider.perform(action: action)
            }
            a2uiAgent.render(
                surfaceId: surfaceId,
                store: store,
                template: ServicesSurface.buildMessages(surfaceId: surfaceId),
                context: servicesContext()
            )
        }
        .onDisappear {
            a2uiAgent.removeActionHandler(surfaceId: surfaceId)
        }
    }

    private func servicesContext() -> String {
        let tunnel = appModel.serviceStatus(for: .tunnel).state.label
        let sync = appModel.serviceStatus(for: .sync).state.label
        let mcp = appModel.serviceStatus(for: .mcp).state.label
        return "tunnel=\(tunnel), sync=\(sync), mcp=\(mcp)"
    }

    private func handleInteraction(_ event: A2UIUserActionEvent) {
        a2uiAgent.handleUserAction(
            surfaceId: surfaceId,
            store: store,
            template: ServicesSurface.buildMessages(surfaceId: surfaceId),
            context: servicesContext(),
            event: event
        )
    }
}

private enum IntegrationsSurface {
    static func buildMessages(surfaceId: String, webhooks: [GitHubWebhook]) -> [NormalizedMsg] {
        var components: [NormalizedComponent] = [
            NormalizedComponent(
                id: "integrations_root",
                type: "Column",
                props: [
                    "children": children([
                        "integrations_title",
                        "integrations_lovable",
                        "integrations_instructions",
                        "integrations_github",
                        "integrations_secrets",
                        "integrations_status"
                    ])
                ]
            ),
            NormalizedComponent(id: "integrations_title", type: "Text", props: [
                "text": .string("Integrations"),
                "style": .string("headline")
            ]),
            NormalizedComponent(id: "integrations_lovable", type: "Column", props: [
                "children": children([
                    "lovable_title",
                    "lovable_tunnel_base",
                    "lovable_mcp_url",
                    "lovable_webhook_url",
                    "lovable_buttons",
                    "lovable_status"
                ])
            ]),
            NormalizedComponent(id: "lovable_title", type: "Text", props: [
                "text": .string("Lovable MCP"),
                "style": .string("subheadline")
            ]),
            NormalizedComponent(id: "lovable_tunnel_base", type: "TextField", props: [
                "label": .string("Tunnel Base URL"),
                "value": binding("integrations.tunnelBaseURL")
            ]),
            NormalizedComponent(id: "lovable_mcp_url", type: "TextField", props: [
                "label": .string("MCP Server URL"),
                "readOnly": .bool(true),
                "value": binding("integrations.mcpServerURL")
            ]),
            NormalizedComponent(id: "lovable_webhook_url", type: "TextField", props: [
                "label": .string("Webhook URL"),
                "readOnly": .bool(true),
                "value": binding("integrations.webhookURL")
            ]),
            NormalizedComponent(id: "lovable_buttons", type: "Row", props: [
                "children": children([
                    "lovable_copy_mcp",
                    "lovable_copy_webhook",
                    "lovable_test_mcp"
                ])
            ]),
            NormalizedComponent(id: "lovable_copy_mcp", type: "Button", props: [
                "label": .string("Copy MCP URL"),
                "action": .string("integrations.copyMcpURL")
            ]),
            NormalizedComponent(id: "lovable_copy_webhook", type: "Button", props: [
                "label": .string("Copy Webhook URL"),
                "action": .string("integrations.copyWebhookURL")
            ]),
            NormalizedComponent(id: "lovable_test_mcp", type: "Button", props: [
                "label": .string("Test MCP Connection"),
                "action": .string("integrations.testMcp")
            ]),
            NormalizedComponent(id: "lovable_status", type: "Text", props: [
                "text": binding("integrations.mcpTestStatus"),
                "style": .string("secondary"),
                "hiddenWhenEmpty": .bool(true)
            ]),
            NormalizedComponent(id: "integrations_instructions", type: "Column", props: [
                "children": children([
                    "instructions_title",
                    "instructions_text",
                    "instructions_copy"
                ])
            ]),
            NormalizedComponent(id: "instructions_title", type: "Text", props: [
                "text": .string("Lovable Instructions"),
                "style": .string("subheadline")
            ]),
            NormalizedComponent(id: "instructions_text", type: "TextField", props: [
                "label": .string("Instructions"),
                "multiline": .bool(true),
                "readOnly": .bool(true),
                "style": .string("monospace"),
                "minHeight": .number(160),
                "value": binding("integrations.lovableInstructions")
            ]),
            NormalizedComponent(id: "instructions_copy", type: "Button", props: [
                "label": .string("Copy Instructions"),
                "action": .string("integrations.copyInstructions")
            ]),
            NormalizedComponent(id: "integrations_github", type: "Column", props: [
                "children": children([
                    "github_title",
                    "github_repo",
                    "github_branch",
                    "github_buttons",
                    "github_webhooks_title",
                    "github_webhooks_list",
                    "github_status"
                ])
            ]),
            NormalizedComponent(id: "github_title", type: "Text", props: [
                "text": .string("GitHub"),
                "style": .string("subheadline")
            ]),
            NormalizedComponent(id: "github_repo", type: "Text", props: [
                "text": binding("integrations.repoLabel")
            ]),
            NormalizedComponent(id: "github_branch", type: "Text", props: [
                "text": binding("integrations.branchLabel")
            ]),
            NormalizedComponent(id: "github_buttons", type: "Row", props: [
                "children": children([
                    "github_create_webhook",
                    "github_refresh_webhooks"
                ])
            ]),
            NormalizedComponent(id: "github_create_webhook", type: "Button", props: [
                "label": .string("Create Webhook"),
                "action": .string("integrations.createWebhook")
            ]),
            NormalizedComponent(id: "github_refresh_webhooks", type: "Button", props: [
                "label": .string("Refresh Webhooks"),
                "action": .string("integrations.refreshWebhooks")
            ]),
            NormalizedComponent(id: "github_webhooks_title", type: "Text", props: [
                "text": .string("Webhooks"),
                "style": .string("subheadline")
            ]),
            NormalizedComponent(id: "github_webhooks_list", type: "Column", props: [
                "children": .array([])
            ]),
            NormalizedComponent(id: "github_status", type: "Text", props: [
                "text": binding("integrations.githubStatus"),
                "style": .string("secondary"),
                "hiddenWhenEmpty": .bool(true)
            ]),
            NormalizedComponent(id: "integrations_secrets", type: "Column", props: [
                "children": children([
                    "secrets_title",
                    "secrets_github_token",
                    "secrets_save_github",
                    "secrets_webhook_token",
                    "secrets_save_webhook",
                    "secrets_mcp_token",
                    "secrets_save_mcp"
                ])
            ]),
            NormalizedComponent(id: "secrets_title", type: "Text", props: [
                "text": .string("Secrets"),
                "style": .string("subheadline")
            ]),
            NormalizedComponent(id: "secrets_github_token", type: "SecureField", props: [
                "label": .string("GitHub Token"),
                "value": binding("integrations.githubToken")
            ]),
            NormalizedComponent(id: "secrets_save_github", type: "Button", props: [
                "label": .string("Save GitHub Token"),
                "action": .string("integrations.saveGitHubToken")
            ]),
            NormalizedComponent(id: "secrets_webhook_token", type: "SecureField", props: [
                "label": .string("Webhook Secret"),
                "value": binding("integrations.webhookSecret")
            ]),
            NormalizedComponent(id: "secrets_save_webhook", type: "Button", props: [
                "label": .string("Save Webhook Secret"),
                "action": .string("integrations.saveWebhookSecret")
            ]),
            NormalizedComponent(id: "secrets_mcp_token", type: "SecureField", props: [
                "label": .string("MCP Bearer Token (optional)"),
                "value": binding("integrations.mcpToken")
            ]),
            NormalizedComponent(id: "secrets_save_mcp", type: "Button", props: [
                "label": .string("Save MCP Token"),
                "action": .string("integrations.saveMcpToken")
            ]),
            NormalizedComponent(id: "integrations_status", type: "Column", props: [
                "children": children([
                    "integrations_status_message",
                    "integrations_status_error"
                ])
            ]),
            NormalizedComponent(id: "integrations_status_message", type: "Text", props: [
                "text": binding("status.message"),
                "style": .string("secondary"),
                "hiddenWhenEmpty": .bool(true)
            ]),
            NormalizedComponent(id: "integrations_status_error", type: "Text", props: [
                "text": binding("status.error"),
                "style": .string("error"),
                "hiddenWhenEmpty": .bool(true)
            ])
        ]

        var webhookChildren: [JSONValue] = []
        if webhooks.isEmpty {
            webhookChildren.append(.string("github_webhooks_empty"))
            components.append(
                NormalizedComponent(id: "github_webhooks_empty", type: "Text", props: [
                    "text": .string("No webhooks configured."),
                    "style": .string("secondary")
                ])
            )
        } else {
            for hook in webhooks {
                let rowId = "github_webhook_row_\(hook.id)"
                let textId = "github_webhook_text_\(hook.id)"
                let spacerId = "github_webhook_spacer_\(hook.id)"
                let deleteId = "github_webhook_delete_\(hook.id)"
                webhookChildren.append(.string(rowId))
                components.append(
                    NormalizedComponent(id: rowId, type: "Row", props: [
                        "children": children([textId, spacerId, deleteId])
                    ])
                )
                components.append(
                    NormalizedComponent(id: textId, type: "Text", props: [
                        "text": .string("Hook \(hook.id) - \(hook.displayURL)")
                    ])
                )
                components.append(NormalizedComponent(id: spacerId, type: "Spacer", props: [:]))
                components.append(
                    NormalizedComponent(id: deleteId, type: "Button", props: [
                        "label": .string("Delete"),
                        "action": .string("integrations.deleteWebhook:\(hook.id)")
                    ])
                )
            }
        }

        if let idx = components.firstIndex(where: { $0.id == "github_webhooks_list" }) {
            var updated = components[idx]
            var props = updated.props
            props["children"] = .array(webhookChildren)
            updated = NormalizedComponent(
                id: updated.id,
                kind: updated.kind,
                props: props,
                childrenRefs: updated.childrenRefs,
                childRef: updated.childRef
            )
            components[idx] = updated
        }

        let info = NormalizedSurfaceInfo(
            surfaceId: surfaceId,
            catalogId: "prune.integrations",
            rootComponentId: "integrations_root",
            protocolVersion: .v09
        )

        return [
            .createSurface(info),
            .updateComponents(surfaceId: surfaceId, components: components)
        ]
    }
}

@MainActor
private final class IntegrationsBindingProvider: A2UIBindingProvider {
    private let appModel: AppModel

    init(appModel: AppModel) {
        self.appModel = appModel
    }

    func stringValue(for key: String) -> String? {
        switch key {
        case "integrations.mcpTestStatus":
            return appModel.mcpTestStatus ?? ""
        case "integrations.githubStatus":
            return appModel.githubStatusMessage ?? ""
        case "integrations.repoLabel":
            return "Repository: \(appModel.config.repoFullName.isEmpty ? "Not set" : appModel.config.repoFullName)"
        case "integrations.branchLabel":
            return "Branch: \(appModel.config.defaultBranch)"
        case "status.message":
            return appModel.statusMessage ?? ""
        case "status.error":
            return appModel.lastErrorMessage ?? ""
        default:
            return nil
        }
    }

    func stringBinding(for key: String) -> Binding<String>? {
        switch key {
        case "integrations.tunnelBaseURL":
            return appModel.binding(\.tunnelBaseURL)
        case "integrations.mcpServerURL":
            return appModel.mcpServerURLBinding
        case "integrations.webhookURL":
            return appModel.webhookURLBinding
        case "integrations.lovableInstructions":
            return appModel.lovableInstructionsBinding
        case "integrations.githubToken":
            return Binding(get: { self.appModel.githubTokenInput }, set: { self.appModel.githubTokenInput = $0 })
        case "integrations.webhookSecret":
            return Binding(get: { self.appModel.webhookSecretInput }, set: { self.appModel.webhookSecretInput = $0 })
        case "integrations.mcpToken":
            return Binding(get: { self.appModel.mcpTokenInput }, set: { self.appModel.mcpTokenInput = $0 })
        default:
            return nil
        }
    }

    func perform(action: String) {
        if action.hasPrefix("integrations.deleteWebhook:") {
            let idString = action.replacingOccurrences(of: "integrations.deleteWebhook:", with: "")
            if let id = Int(idString) {
                appModel.deleteGitHubWebhook(id: id)
            }
            return
        }

        switch action {
        case "integrations.copyMcpURL":
            appModel.copyMcpURL()
        case "integrations.copyWebhookURL":
            appModel.copyWebhookURL()
        case "integrations.testMcp":
            appModel.testMcpConnection()
        case "integrations.copyInstructions":
            appModel.copyLovableInstructions()
        case "integrations.createWebhook":
            appModel.createGitHubWebhook()
        case "integrations.refreshWebhooks":
            appModel.refreshGitHubWebhooks()
        case "integrations.saveGitHubToken":
            appModel.saveGitHubToken()
        case "integrations.saveWebhookSecret":
            appModel.saveWebhookSecret()
        case "integrations.saveMcpToken":
            appModel.saveMcpToken()
        default:
            break
        }
    }
}

struct IntegrationsView: View {
    @EnvironmentObject private var appModel: AppModel
    @EnvironmentObject private var a2uiAgent: A2UIAgent
    @StateObject private var store = NormalizedSurfaceStore()
    private let surfaceId = "prune_integrations"

    var body: some View {
        let bindingProvider = IntegrationsBindingProvider(appModel: appModel)
        ScrollView {
            A2UISurfaceView(
                store: store,
                surfaceId: surfaceId,
                bindingProvider: bindingProvider,
                interactionHandler: { event in
                    handleInteraction(event)
                }
            )
            .padding()
        }
        .onAppear {
            a2uiAgent.registerActionHandler(surfaceId: surfaceId) { action, _ in
                bindingProvider.perform(action: action)
            }
            renderIntegrations()
        }
        .onChange(of: appModel.webhooks.map(\.id)) { _, _ in
            renderIntegrations()
        }
        .onDisappear {
            a2uiAgent.removeActionHandler(surfaceId: surfaceId)
        }
    }

    private func renderIntegrations() {
        a2uiAgent.render(
            surfaceId: surfaceId,
            store: store,
            template: IntegrationsSurface.buildMessages(
                surfaceId: surfaceId,
                webhooks: appModel.webhooks
            ),
            context: "repo=\(appModel.config.repoFullName), webhooks=\(appModel.webhooks.count)"
        )
    }

    private func handleInteraction(_ event: A2UIUserActionEvent) {
        a2uiAgent.handleUserAction(
            surfaceId: surfaceId,
            store: store,
            template: IntegrationsSurface.buildMessages(surfaceId: surfaceId, webhooks: appModel.webhooks),
            context: "repo=\(appModel.config.repoFullName), webhooks=\(appModel.webhooks.count)",
            event: event
        )
    }
}

private enum DiagnosticsInputMode: String, CaseIterable {
    case fixture
    case file
    case live

    var label: String {
        switch self {
        case .fixture:
            return "Fixture"
        case .file:
            return "JSONL File"
        case .live:
            return "Live Stream"
        }
    }
}

private enum DiagnosticsFixtureVersion: String, CaseIterable {
    case v09
    case v08

    var label: String {
        switch self {
        case .v09:
            return "v0.9"
        case .v08:
            return "v0.8"
        }
    }
}

private struct DiagnosticsReportSummary: Equatable {
    let surfaceId: String
    let protocolVersion: String?
    let rootComponentId: String?
    let componentCount: Int
    let resolvedText: String?
    let dataModelJSON: String
    let errors: [String]
}

private enum DiagnosticsSurface {
    static func buildMessages(
        surfaceId: String,
        inputMode: DiagnosticsInputMode
    ) -> [NormalizedMsg] {
        var components: [NormalizedComponent] = []

        let rootChildren: [String] = [
            "diag_title",
            "diag_subtitle",
            "diag_input",
            "diag_status",
            "diag_output"
        ]

        components.append(contentsOf: [
            NormalizedComponent(
                id: "diag_root",
                type: "Column",
                props: [
                    "children": children(rootChildren)
                ]
            ),
            NormalizedComponent(id: "diag_title", type: "Text", props: [
                "text": .string("A2UI Inception Diagnostics"),
                "style": .string("headline")
            ]),
            NormalizedComponent(id: "diag_subtitle", type: "Text", props: [
                "text": .string("Run fixtures, load JSONL, or stream live messages into A2UIRuntime."),
                "style": .string("secondary")
            ])
        ])

        var inputChildren: [String] = [
            "diag_input_title",
            "diag_input_mode"
        ]

        components.append(
            NormalizedComponent(id: "diag_input", type: "Column", props: [
                "children": .array([])
            ])
        )
        components.append(
            NormalizedComponent(id: "diag_input_title", type: "Text", props: [
                "text": .string("Input"),
                "style": .string("subheadline")
            ])
        )
        components.append(
            NormalizedComponent(id: "diag_input_mode", type: "Select", props: [
                "label": .string("Mode"),
                "value": binding("diag.inputMode"),
                "options": .array(DiagnosticsInputMode.allCases.map { mode in
                    .object(["label": .string(mode.label), "value": .string(mode.rawValue)])
                })
            ])
        )

        switch inputMode {
        case .fixture:
            inputChildren.append(contentsOf: [
                "diag_fixture_version",
                "diag_fixture_buttons"
            ])
            components.append(
                NormalizedComponent(id: "diag_fixture_version", type: "Select", props: [
                    "label": .string("Fixture Version"),
                    "value": binding("diag.fixtureVersion"),
                    "options": .array(DiagnosticsFixtureVersion.allCases.map { version in
                        .object(["label": .string(version.label), "value": .string(version.rawValue)])
                    })
                ])
            )
            components.append(
                NormalizedComponent(id: "diag_fixture_buttons", type: "Row", props: [
                    "children": children([
                        "diag_run_fixture",
                        "diag_reset_fixture"
                    ])
                ])
            )
            components.append(
                NormalizedComponent(id: "diag_run_fixture", type: "Button", props: [
                    "label": .string("Run Fixture"),
                    "action": .string("diag.runFixture"),
                    "variant": .string("primary"),
                    "disabled": binding("diag.running")
                ])
            )
            components.append(
                NormalizedComponent(id: "diag_reset_fixture", type: "Button", props: [
                    "label": .string("Reset"),
                    "action": .string("diag.reset")
                ])
            )
        case .file:
            inputChildren.append(contentsOf: [
                "diag_file_path",
                "diag_file_buttons",
                "diag_file_note"
            ])
            components.append(
                NormalizedComponent(id: "diag_file_path", type: "TextField", props: [
                    "label": .string("JSONL file path"),
                    "value": binding("diag.filePath")
                ])
            )
            components.append(
                NormalizedComponent(id: "diag_file_buttons", type: "Row", props: [
                    "children": children([
                        "diag_file_browse",
                        "diag_file_load",
                        "diag_file_reset"
                    ])
                ])
            )
            components.append(
                NormalizedComponent(id: "diag_file_browse", type: "Button", props: [
                    "label": .string("Browse"),
                    "action": .string("diag.browse")
                ])
            )
            components.append(
                NormalizedComponent(id: "diag_file_load", type: "Button", props: [
                    "label": .string("Load File"),
                    "action": .string("diag.loadFile"),
                    "variant": .string("primary")
                ])
            )
            components.append(
                NormalizedComponent(id: "diag_file_reset", type: "Button", props: [
                    "label": .string("Reset"),
                    "action": .string("diag.reset")
                ])
            )
            components.append(
                NormalizedComponent(id: "diag_file_note", type: "Text", props: [
                    "text": .string("Each line should be a JSON object (A2UI message envelope)."),
                    "style": .string("secondary")
                ])
            )
        case .live:
            inputChildren.append(contentsOf: [
                "diag_stream_url",
                "diag_stream_buttons",
                "diag_stream_note"
            ])
            components.append(
                NormalizedComponent(id: "diag_stream_url", type: "TextField", props: [
                    "label": .string("Stream URL"),
                    "value": binding("diag.streamURL")
                ])
            )
            components.append(
                NormalizedComponent(id: "diag_stream_buttons", type: "Row", props: [
                    "children": children([
                        "diag_stream_toggle",
                        "diag_stream_reset"
                    ])
                ])
            )
            components.append(
                NormalizedComponent(id: "diag_stream_toggle", type: "Button", props: [
                    "label": binding("diag.streamButtonLabel"),
                    "action": .string("diag.toggleStream"),
                    "variant": .string("primary")
                ])
            )
            components.append(
                NormalizedComponent(id: "diag_stream_reset", type: "Button", props: [
                    "label": .string("Reset"),
                    "action": .string("diag.reset")
                ])
            )
            components.append(
                NormalizedComponent(id: "diag_stream_note", type: "Text", props: [
                    "text": .string("Stream expects newline-delimited JSON objects over HTTP."),
                    "style": .string("secondary")
                ])
            )
        }

        if let inputIndex = components.firstIndex(where: { $0.id == "diag_input" }) {
            var updated = components[inputIndex]
            var props = updated.props
            props["children"] = children(inputChildren)
            updated = NormalizedComponent(
                id: updated.id,
                kind: updated.kind,
                props: props,
                childrenRefs: updated.childrenRefs,
                childRef: updated.childRef
            )
            components[inputIndex] = updated
        }

        components.append(
            NormalizedComponent(id: "diag_status", type: "Column", props: [
                "children": children([
                    "diag_status_message",
                    "diag_error_message",
                    "diag_activity"
                ])
            ])
        )
        components.append(
            NormalizedComponent(id: "diag_status_message", type: "Text", props: [
                "text": binding("diag.status"),
                "style": .string("secondary"),
                "hiddenWhenEmpty": .bool(true)
            ])
        )
        components.append(
            NormalizedComponent(id: "diag_error_message", type: "Text", props: [
                "text": binding("diag.error"),
                "style": .string("error"),
                "hiddenWhenEmpty": .bool(true)
            ])
        )
        components.append(
            NormalizedComponent(id: "diag_activity", type: "Text", props: [
                "text": binding("diag.activity"),
                "style": .string("secondary"),
                "hiddenWhenEmpty": .bool(true)
            ])
        )

        components.append(
            NormalizedComponent(id: "diag_output", type: "Column", props: [
                "children": children([
                    "diag_output_title",
                    "diag_output_summary"
                ])
            ])
        )
        components.append(
            NormalizedComponent(id: "diag_output_title", type: "Text", props: [
                "text": .string("Output"),
                "style": .string("subheadline")
            ])
        )
        components.append(
            NormalizedComponent(id: "diag_output_summary", type: "TextField", props: [
                "label": .string("Summary"),
                "multiline": .bool(true),
                "readOnly": .bool(true),
                "style": .string("monospace"),
                "minHeight": .number(200),
                "value": binding("diag.reportSummary")
            ])
        )

        let info = NormalizedSurfaceInfo(
            surfaceId: surfaceId,
            catalogId: "prune.a2ui.diagnostics",
            rootComponentId: "diag_root",
            protocolVersion: .v09
        )

        return [
            .createSurface(info),
            .updateComponents(surfaceId: surfaceId, components: components)
        ]
    }
}

@MainActor
private final class DiagnosticsBindingProvider: A2UIBindingProvider {
    private let inputMode: Binding<String>
    private let fixtureVersion: Binding<String>
    private let filePath: Binding<String>
    private let streamURL: Binding<String>
    private let statusMessage: () -> String?
    private let errorMessage: () -> String?
    private let activityMessage: () -> String?
    private let isRunning: () -> Bool
    private let reportSummaryProvider: () -> String
    private let streamLabelProvider: () -> String
    private let actions: (String) -> Void

    init(
        inputMode: Binding<String>,
        fixtureVersion: Binding<String>,
        filePath: Binding<String>,
        streamURL: Binding<String>,
        statusMessage: @escaping () -> String?,
        errorMessage: @escaping () -> String?,
        activityMessage: @escaping () -> String?,
        isRunning: @escaping () -> Bool,
        reportSummary: @escaping () -> String,
        streamLabel: @escaping () -> String,
        actions: @escaping (String) -> Void
    ) {
        self.inputMode = inputMode
        self.fixtureVersion = fixtureVersion
        self.filePath = filePath
        self.streamURL = streamURL
        self.statusMessage = statusMessage
        self.errorMessage = errorMessage
        self.activityMessage = activityMessage
        self.isRunning = isRunning
        self.reportSummaryProvider = reportSummary
        self.streamLabelProvider = streamLabel
        self.actions = actions
    }

    func stringBinding(for key: String) -> Binding<String>? {
        switch key {
        case "diag.inputMode":
            return inputMode
        case "diag.fixtureVersion":
            return fixtureVersion
        case "diag.filePath":
            return filePath
        case "diag.streamURL":
            return streamURL
        case "diag.reportSummary":
            return Binding(get: { self.reportSummary() }, set: { _ in })
        default:
            return nil
        }
    }

    func stringValue(for key: String) -> String? {
        switch key {
        case "diag.status":
            return statusMessage() ?? ""
        case "diag.error":
            return errorMessage() ?? ""
        case "diag.activity":
            return activityMessage() ?? ""
        case "diag.streamButtonLabel":
            return streamLabel()
        default:
            return nil
        }
    }

    func boolValue(for key: String) -> Bool? {
        switch key {
        case "diag.running":
            return isRunning()
        default:
            return nil
        }
    }

    func perform(action: String) {
        actions(action)
    }

    private func reportSummary() -> String {
        return reportSummaryProvider()
    }

    private func streamLabel() -> String {
        return streamLabelProvider()
    }
}

struct A2UIDiagnosticsView: View {
    @EnvironmentObject private var a2uiAgent: A2UIAgent
    @State private var inputMode: DiagnosticsInputMode = .fixture
    @State private var fixtureVersion: DiagnosticsFixtureVersion = .v09
    @State private var filePath: String = ""
    @State private var isFileImporterPresented = false
    @State private var streamURLText: String = "http://localhost:47802/a2ui"
    @State private var statusMessage: String?
    @State private var errorMessage: String?
    @State private var report: DiagnosticsReportSummary?
    @State private var errors: [String] = []
    @State private var isRunning = false
    @State private var isStreaming = false
    @State private var streamTask: Task<Void, Never>?
    @State private var fixtureStore = NormalizedSurfaceStore()
    @StateObject private var uiStore = NormalizedSurfaceStore()
    private let surfaceId = "prune_a2ui_diagnostics"

    var body: some View {
        let bindingProvider = DiagnosticsBindingProvider(
            inputMode: inputModeBinding,
            fixtureVersion: fixtureVersionBinding,
            filePath: $filePath,
            streamURL: $streamURLText,
            statusMessage: { statusMessage },
            errorMessage: { errorMessage },
            activityMessage: { activityMessage },
            isRunning: { isRunning },
            reportSummary: { reportSummaryText },
            streamLabel: { streamButtonLabel },
            actions: handleAction
        )
        ScrollView {
            A2UISurfaceView(
                store: uiStore,
                surfaceId: surfaceId,
                bindingProvider: bindingProvider,
                interactionHandler: { event in
                    handleInteraction(event)
                }
            )
            .padding()
        }
        .fileImporter(
            isPresented: $isFileImporterPresented,
            allowedContentTypes: [.json, .plainText, .text]
        ) { result in
            switch result {
            case .success(let url):
                let needsAccess = url.startAccessingSecurityScopedResource()
                if needsAccess {
                    defer { url.stopAccessingSecurityScopedResource() }
                    filePath = url.path
                    loadJSONL(from: url)
                } else {
                    filePath = url.path
                    loadJSONL(from: url)
                }
            case .failure(let error):
                errorMessage = "File import failed: \(error.localizedDescription)"
                renderSurface()
            }
        }
        .onAppear {
            a2uiAgent.registerActionHandler(surfaceId: surfaceId) { action, _ in
                bindingProvider.perform(action: action)
            }
            renderSurface()
            guard report == nil, !isRunning, !isStreaming else { return }
            DispatchQueue.main.async {
                runFixture()
            }
        }
        .onChange(of: inputMode) { _, _ in
            renderSurface()
        }
        .onChange(of: fixtureVersion) { _, _ in
            renderSurface()
        }
        .onChange(of: isRunning) { _, _ in
            renderSurface()
        }
        .onChange(of: isStreaming) { _, _ in
            renderSurface()
        }
        .onDisappear {
            stopStreaming()
            a2uiAgent.removeActionHandler(surfaceId: surfaceId)
        }
    }

    private var inputModeBinding: Binding<String> {
        Binding(
            get: { inputMode.rawValue },
            set: { rawValue in
                if let mode = DiagnosticsInputMode(rawValue: rawValue) {
                    inputMode = mode
                }
            }
        )
    }

    private var fixtureVersionBinding: Binding<String> {
        Binding(
            get: { fixtureVersion.rawValue },
            set: { rawValue in
                if let version = DiagnosticsFixtureVersion(rawValue: rawValue) {
                    fixtureVersion = version
                }
            }
        )
    }

    private var activityMessage: String? {
        if isRunning {
            return "Running..."
        }
        if isStreaming {
            return "Streaming..."
        }
        return nil
    }

    @MainActor
    private func handleAction(_ action: String) {
        switch action {
        case "diag.runFixture":
            runFixture()
        case "diag.reset":
            resetState()
        case "diag.browse":
            isFileImporterPresented = true
        case "diag.loadFile":
            loadJSONLFromPath()
        case "diag.toggleStream":
            if isStreaming {
                stopStreaming()
            } else {
                startStreaming()
            }
        default:
            break
        }
    }

    private var reportSummaryText: String {
        guard let report else { return "No A2UI output yet." }
        var lines: [String] = []
        lines.append("Surface ID: \(report.surfaceId)")
        lines.append("Protocol: \(report.protocolVersion ?? "unknown")")
        lines.append("Root component: \(report.rootComponentId ?? "unknown")")
        lines.append("Components: \(report.componentCount)")
        lines.append("")
        lines.append("Resolved Text:")
        lines.append(report.resolvedText ?? "No bound text resolved.")
        lines.append("")
        lines.append("Data Model:")
        lines.append(report.dataModelJSON)
        if !report.errors.isEmpty {
            lines.append("")
            lines.append("Adapter Errors:")
            lines.append(contentsOf: report.errors)
        }
        return lines.joined(separator: "\n")
    }

    private var streamButtonLabel: String {
        isStreaming ? "Stop Stream" : "Start Stream"
    }

    @MainActor
    private func runFixture() {
        let lines: [String]
        let preferred: A2UIProtocolVersion
        switch fixtureVersion {
        case .v09:
            lines = Self.v09FixtureLines
            preferred = .v09
        case .v08:
            lines = Self.v08FixtureLines
            preferred = .v08
        }
        runLines(lines, label: "Loaded \(fixtureVersion.label) fixture.", preferredVersion: preferred)
    }

    @MainActor
    private func loadJSONLFromPath() {
        let trimmed = filePath.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else {
            errorMessage = "Enter a JSONL file path."
            renderSurface()
            return
        }
        loadJSONL(from: URL(fileURLWithPath: trimmed))
    }

    @MainActor
    private func loadJSONL(from url: URL) {
        do {
            let content = try String(contentsOf: url, encoding: .utf8)
            let lines = content.split(whereSeparator: \.isNewline).map(String.init)
            guard !lines.isEmpty else {
                errorMessage = "No JSONL lines found in \(url.lastPathComponent)."
                renderSurface()
                return
            }
            runLines(
                lines,
                label: "Loaded \(lines.count) lines from \(url.lastPathComponent).",
                preferredVersion: nil
            )
        } catch {
            errorMessage = "Failed to read file: \(error.localizedDescription)"
            renderSurface()
        }
    }

    @MainActor
    private func startStreaming() {
        stopStreaming()
        guard let url = URL(string: streamURLText), url.scheme != nil else {
            errorMessage = "Invalid stream URL."
            renderSurface()
            return
        }

        resetState()
        isStreaming = true
        statusMessage = "Connecting to stream..."
        renderSurface()

        let adapter = A2UIProtocolAdapter(enableV09: true)
        streamTask = Task { @MainActor in
            do {
                let (bytes, _) = try await URLSession.shared.bytes(from: url)
                statusMessage = "Streaming..."
                renderSurface()
                for try await line in bytes.lines {
                    if Task.isCancelled {
                        break
                    }
                    let trimmed = line.trimmingCharacters(in: .whitespacesAndNewlines)
                    guard !trimmed.isEmpty else { continue }
                    let messages = adapter.decode(line: trimmed)
                    applyMessages(messages)
                }
                statusMessage = Task.isCancelled ? "Stream stopped." : "Stream ended."
            } catch {
                statusMessage = "Stream error: \(error.localizedDescription)"
            }
            isStreaming = false
            renderSurface()
        }
    }

    @MainActor
    private func stopStreaming() {
        streamTask?.cancel()
        streamTask = nil
        if isStreaming {
            isStreaming = false
            statusMessage = "Stream stopped."
            renderSurface()
        }
    }

    @MainActor
    private func runLines(
        _ lines: [String],
        label: String,
        preferredVersion: A2UIProtocolVersion?
    ) {
        isRunning = true
        resetState()
        statusMessage = label
        renderSurface()

        let adapter = A2UIProtocolAdapter(enableV09: true, preferredVersion: preferredVersion)
        for line in lines {
            let trimmed = line.trimmingCharacters(in: .whitespacesAndNewlines)
            guard !trimmed.isEmpty else { continue }
            let messages = adapter.decode(line: trimmed)
            applyMessages(messages)
        }

        isRunning = false
        renderSurface()
    }

    @MainActor
    private func applyMessages(_ messages: [NormalizedMsg]) {
        for message in messages {
            switch message {
            case .error(let message):
                errors.append(message)
            default:
                fixtureStore.apply(message)
            }
        }
        updateReport()
    }

    @MainActor
    private func updateReport() {
        guard let surfaceId = fixtureStore.surfaces.keys.sorted().first,
              let surface = fixtureStore.surfaces[surfaceId] else {
            report = nil
            renderSurface()
            return
        }

        let rootId = fixtureStore.rootComponentId(for: surfaceId)
        var resolvedText: String?
        if let rootId,
           let props = fixtureStore.resolvedProps(surfaceId: surfaceId, componentId: rootId),
           let textValue = props["text"] {
            resolvedText = textValue.stringValue ?? prettyJSON(textValue)
        }

        report = DiagnosticsReportSummary(
            surfaceId: surfaceId,
            protocolVersion: surface.info.protocolVersion?.rawValue,
            rootComponentId: rootId,
            componentCount: surface.components.count,
            resolvedText: resolvedText,
            dataModelJSON: prettyJSON(surface.dataModel),
            errors: errors
        )
        renderSurface()
    }

    @MainActor
    private func resetState() {
        fixtureStore = NormalizedSurfaceStore()
        errors = []
        report = nil
        errorMessage = nil
        statusMessage = nil
        renderSurface()
    }

    private func prettyJSON(_ value: JSONValue) -> String {
        let anyValue = value.toAny()
        if JSONSerialization.isValidJSONObject(anyValue),
           let data = try? JSONSerialization.data(
            withJSONObject: anyValue,
            options: [.prettyPrinted, .sortedKeys]
           ),
           let text = String(data: data, encoding: .utf8) {
            return text
        }
        return String(describing: anyValue)
    }

    private static let v09FixtureLines = [
        """
        {"createSurface":{"surfaceId":"inception","catalogId":"demo","rootComponentId":"root"}}
        """,
        """
        {"updateComponents":{"surfaceId":"inception","components":[{"id":"root","component":"Text","text":{"path":"/question"}}]}}
        """,
        """
        {"updateDataModel":{"surfaceId":"inception","updates":[{"path":"/question","value":"Hello from A2UI v0.9"}]}}
        """
    ]

    private static let v08FixtureLines = [
        """
        {"beginRendering":{"surfaceId":"inception","catalogId":"demo","rootComponentId":"root"}}
        """,
        """
        {"surfaceUpdate":{"surfaceId":"inception","components":[{"id":"root","component":{"Text":{"text":{"path":"/question"}}}}]}}
        """,
        """
        {"dataModelUpdate":{"surfaceId":"inception","contents":[{"path":"/question","value":{"literalString":"Hello from A2UI v0.8"}}]}}
        """
    ]

    @MainActor
    private func renderSurface() {
        a2uiAgent.render(
            surfaceId: surfaceId,
            store: uiStore,
            template: DiagnosticsSurface.buildMessages(
                surfaceId: surfaceId,
                inputMode: inputMode
            ),
            context: "mode=\(inputMode.rawValue), fixture=\(fixtureVersion.rawValue)"
        )
    }

    private func handleInteraction(_ event: A2UIUserActionEvent) {
        a2uiAgent.handleUserAction(
            surfaceId: surfaceId,
            store: uiStore,
            template: DiagnosticsSurface.buildMessages(surfaceId: surfaceId, inputMode: inputMode),
            context: "mode=\(inputMode.rawValue), fixture=\(fixtureVersion.rawValue)",
            event: event
        )
    }
}

private enum HelpSurface {
    static func buildMessages(surfaceId: String, missingGit: Bool) -> [NormalizedMsg] {
        var childrenIds: [String] = [
            "help_title"
        ]
        if missingGit {
            childrenIds.append("help_git_required")
        }
        childrenIds.append(contentsOf: [
            "help_prereq",
            "help_quickstart",
            "help_troubleshooting",
            "help_diagnostics",
            "help_status"
        ])

        var components: [NormalizedComponent] = [
            NormalizedComponent(
                id: "help_root",
                type: "Column",
                props: [
                    "children": children(childrenIds)
                ]
            ),
            NormalizedComponent(id: "help_title", type: "Text", props: [
                "text": .string("Help"),
                "style": .string("headline")
            ])
        ]

        if missingGit {
            components.append(contentsOf: [
                NormalizedComponent(id: "help_git_required", type: "Column", props: [
                    "children": children([
                        "help_git_title",
                        "help_git_reason",
                        "help_git_buttons"
                    ])
                ]),
                NormalizedComponent(id: "help_git_title", type: "Text", props: [
                    "text": .string("Git Required"),
                    "style": .string("subheadline")
                ]),
                NormalizedComponent(id: "help_git_reason", type: "Text", props: [
                    "text": binding("help.gitReason")
                ]),
                NormalizedComponent(id: "help_git_buttons", type: "Row", props: [
                    "children": children([
                        "help_install_clt",
                        "help_recheck_git"
                    ])
                ]),
                NormalizedComponent(id: "help_install_clt", type: "Button", props: [
                    "label": .string("Install Command Line Tools"),
                    "action": .string("help.installCLT")
                ]),
                NormalizedComponent(id: "help_recheck_git", type: "Button", props: [
                    "label": .string("Recheck"),
                    "action": .string("help.recheckGit")
                ])
            ])
        }

        components.append(contentsOf: [
            NormalizedComponent(id: "help_prereq", type: "Column", props: [
                "children": children([
                    "help_prereq_title",
                    "help_prereq_1",
                    "help_prereq_2",
                    "help_prereq_3",
                    "help_prereq_4",
                    "help_prereq_5"
                ])
            ]),
            NormalizedComponent(id: "help_prereq_title", type: "Text", props: [
                "text": .string("Prerequisites"),
                "style": .string("subheadline")
            ]),
            NormalizedComponent(id: "help_prereq_1", type: "Text", props: [
                "text": .string("macOS 13 or newer.")
            ]),
            NormalizedComponent(id: "help_prereq_2", type: "Text", props: [
                "text": .string("Git installed (Xcode Command Line Tools). Required to mirror repos and detect remotes.")
            ]),
            NormalizedComponent(id: "help_prereq_3", type: "Text", props: [
                "text": .string("If Git is missing, run: xcode-select --install")
            ]),
            NormalizedComponent(id: "help_prereq_4", type: "Text", props: [
                "text": .string("Network access for tunnel + webhooks.")
            ]),
            NormalizedComponent(id: "help_prereq_5", type: "Text", props: [
                "text": .string("Optional: GitHub token + webhook secret for GitHub sync.")
            ]),
            NormalizedComponent(id: "help_quickstart", type: "Column", props: [
                "children": children([
                    "help_quickstart_title",
                    "help_quickstart_1",
                    "help_quickstart_2",
                    "help_quickstart_3",
                    "help_quickstart_4",
                    "help_quickstart_5",
                    "help_quickstart_6"
                ])
            ]),
            NormalizedComponent(id: "help_quickstart_title", type: "Text", props: [
                "text": .string("Quickstart"),
                "style": .string("subheadline")
            ]),
            NormalizedComponent(id: "help_quickstart_1", type: "Text", props: [
                "text": .string("1. Install Prune.app (Install.command); bundled dependencies install on first launch.")
            ]),
            NormalizedComponent(id: "help_quickstart_2", type: "Text", props: [
                "text": .string("2. Click Start to bring up the tunnel and services.")
            ]),
            NormalizedComponent(id: "help_quickstart_3", type: "Text", props: [
                "text": .string("3. Copy the MCP Server URL.")
            ]),
            NormalizedComponent(id: "help_quickstart_4", type: "Text", props: [
                "text": .string("4. In Lovable: Settings -> Connectors -> Personal connectors -> New MCP server.")
            ]),
            NormalizedComponent(id: "help_quickstart_5", type: "Text", props: [
                "text": .string("5. Connect Lovable project to GitHub (default branch sync).")
            ]),
            NormalizedComponent(id: "help_quickstart_6", type: "Text", props: [
                "text": .string("6. Confirm Last Indexed SHA updates after edits.")
            ]),
            NormalizedComponent(id: "help_troubleshooting", type: "Column", props: [
                "children": children([
                    "help_troubleshooting_title",
                    "help_troubleshooting_1",
                    "help_troubleshooting_2",
                    "help_troubleshooting_3",
                    "help_troubleshooting_4"
                ])
            ]),
            NormalizedComponent(id: "help_troubleshooting_title", type: "Text", props: [
                "text": .string("Troubleshooting"),
                "style": .string("subheadline")
            ]),
            NormalizedComponent(id: "help_troubleshooting_1", type: "Text", props: [
                "text": .string("MCP not reachable: confirm tunnel is running and URL is copied exactly.")
            ]),
            NormalizedComponent(id: "help_troubleshooting_2", type: "Text", props: [
                "text": .string("Webhook failing: ensure GitHub token and webhook secret are stored.")
            ]),
            NormalizedComponent(id: "help_troubleshooting_3", type: "Text", props: [
                "text": .string("Index not updating: check sync service status and logs.")
            ]),
            NormalizedComponent(id: "help_troubleshooting_4", type: "Text", props: [
                "text": .string("Tunnel expired: stop and start to refresh.")
            ]),
            NormalizedComponent(id: "help_diagnostics", type: "Column", props: [
                "children": children([
                    "help_diagnostics_title",
                    "help_diagnostics_button",
                    "help_diagnostics_last"
                ])
            ]),
            NormalizedComponent(id: "help_diagnostics_title", type: "Text", props: [
                "text": .string("Diagnostics"),
                "style": .string("subheadline")
            ]),
            NormalizedComponent(id: "help_diagnostics_button", type: "Button", props: [
                "label": .string("Export Diagnostics"),
                "action": .string("help.exportDiagnostics")
            ]),
            NormalizedComponent(id: "help_diagnostics_last", type: "Text", props: [
                "text": binding("help.lastDiagnostics"),
                "style": .string("secondary"),
                "hiddenWhenEmpty": .bool(true)
            ]),
            NormalizedComponent(id: "help_status", type: "Column", props: [
                "children": children([
                    "help_status_message",
                    "help_status_error"
                ])
            ]),
            NormalizedComponent(id: "help_status_message", type: "Text", props: [
                "text": binding("status.message"),
                "style": .string("secondary"),
                "hiddenWhenEmpty": .bool(true)
            ]),
            NormalizedComponent(id: "help_status_error", type: "Text", props: [
                "text": binding("status.error"),
                "style": .string("error"),
                "hiddenWhenEmpty": .bool(true)
            ])
        ])

        let info = NormalizedSurfaceInfo(
            surfaceId: surfaceId,
            catalogId: "prune.help",
            rootComponentId: "help_root",
            protocolVersion: .v09
        )

        return [
            .createSurface(info),
            .updateComponents(surfaceId: surfaceId, components: components)
        ]
    }
}

@MainActor
private final class HelpBindingProvider: A2UIBindingProvider {
    private let appModel: AppModel

    init(appModel: AppModel) {
        self.appModel = appModel
    }

    func stringValue(for key: String) -> String? {
        switch key {
        case "help.gitReason":
            if case let .missing(reason) = appModel.gitAvailability {
                return reason
            }
            return ""
        case "help.lastDiagnostics":
            if let path = appModel.lastDiagnosticsPath {
                return "Last export: \(path.path)"
            }
            return ""
        case "status.message":
            return appModel.statusMessage ?? ""
        case "status.error":
            return appModel.lastErrorMessage ?? ""
        default:
            return nil
        }
    }

    func perform(action: String) {
        switch action {
        case "help.installCLT":
            appModel.installCommandLineTools()
        case "help.recheckGit":
            appModel.refreshGitAvailability()
        case "help.exportDiagnostics":
            appModel.exportDiagnostics()
        default:
            break
        }
    }
}

private enum PrivacySurface {
    static func buildMessages(surfaceId: String) -> [NormalizedMsg] {
        let components: [NormalizedComponent] = [
            NormalizedComponent(
                id: "privacy_root",
                type: "Column",
                props: [
                    "children": children([
                        "privacy_title",
                        "privacy_toggle",
                        "privacy_note",
                        "privacy_link"
                    ])
                ]
            ),
            NormalizedComponent(id: "privacy_title", type: "Text", props: [
                "text": .string("Privacy"),
                "style": .string("headline")
            ]),
            NormalizedComponent(id: "privacy_toggle", type: "Toggle", props: [
                "label": .string("Share anonymous usage analytics"),
                "value": binding("privacy.analyticsOptIn")
            ]),
            NormalizedComponent(id: "privacy_note", type: "Text", props: [
                "text": .string("Analytics are opt-in and never include code, prompts, or repository contents."),
                "style": .string("secondary")
            ]),
            NormalizedComponent(id: "privacy_link", type: "Button", props: [
                "label": .string("Privacy Policy"),
                "action": .string("privacy.openPolicy")
            ])
        ]

        let info = NormalizedSurfaceInfo(
            surfaceId: surfaceId,
            catalogId: "prune.privacy",
            rootComponentId: "privacy_root",
            protocolVersion: .v09
        )

        return [
            .createSurface(info),
            .updateComponents(surfaceId: surfaceId, components: components)
        ]
    }
}

@MainActor
private final class PrivacyBindingProvider: A2UIBindingProvider {
    private let appModel: AppModel

    init(appModel: AppModel) {
        self.appModel = appModel
    }

    func boolBinding(for key: String) -> Binding<Bool>? {
        switch key {
        case "privacy.analyticsOptIn":
            return Binding(
                get: { self.appModel.analyticsOptIn },
                set: { self.appModel.analyticsOptIn = $0 }
            )
        default:
            return nil
        }
    }

    func perform(action: String) {
        switch action {
        case "privacy.openPolicy":
            if let url = URL(string: "https://prune.dev/privacy") {
                NSWorkspace.shared.open(url)
            }
        default:
            break
        }
    }
}

struct HelpView: View {
    @EnvironmentObject private var appModel: AppModel
    @EnvironmentObject private var a2uiAgent: A2UIAgent
    @StateObject private var store = NormalizedSurfaceStore()
    private let surfaceId = "prune_help"

    var body: some View {
        let bindingProvider = HelpBindingProvider(appModel: appModel)
        ScrollView {
            A2UISurfaceView(
                store: store,
                surfaceId: surfaceId,
                bindingProvider: bindingProvider,
                interactionHandler: { event in
                    handleInteraction(event)
                }
            )
            .padding()
        }
        .onAppear {
            a2uiAgent.registerActionHandler(surfaceId: surfaceId) { action, _ in
                bindingProvider.perform(action: action)
            }
            appModel.refreshGitAvailability()
            renderHelp()
        }
        .onChange(of: appModel.gitAvailability) { _, _ in
            renderHelp()
        }
        .onDisappear {
            a2uiAgent.removeActionHandler(surfaceId: surfaceId)
        }
    }

    private var isMissingGit: Bool {
        if case .missing = appModel.gitAvailability {
            return true
        }
        return false
    }

    private func renderHelp() {
        a2uiAgent.render(
            surfaceId: surfaceId,
            store: store,
            template: HelpSurface.buildMessages(surfaceId: surfaceId, missingGit: isMissingGit),
            context: "gitMissing=\(isMissingGit)"
        )
    }

    private func handleInteraction(_ event: A2UIUserActionEvent) {
        a2uiAgent.handleUserAction(
            surfaceId: surfaceId,
            store: store,
            template: HelpSurface.buildMessages(surfaceId: surfaceId, missingGit: isMissingGit),
            context: "gitMissing=\(isMissingGit)",
            event: event
        )
    }
}

struct PrivacyView: View {
    @EnvironmentObject private var appModel: AppModel
    @EnvironmentObject private var a2uiAgent: A2UIAgent
    @StateObject private var store = NormalizedSurfaceStore()
    private let surfaceId = "prune_privacy"

    var body: some View {
        let bindingProvider = PrivacyBindingProvider(appModel: appModel)
        A2UISurfaceView(
            store: store,
            surfaceId: surfaceId,
            bindingProvider: bindingProvider,
            interactionHandler: { event in
                handleInteraction(event)
            }
        )
        .padding()
        .onAppear {
            a2uiAgent.registerActionHandler(surfaceId: surfaceId) { action, _ in
                bindingProvider.perform(action: action)
            }
            a2uiAgent.render(
                surfaceId: surfaceId,
                store: store,
                template: PrivacySurface.buildMessages(surfaceId: surfaceId),
                context: "analyticsOptIn=\(appModel.analyticsOptIn)"
            )
        }
        .onDisappear {
            a2uiAgent.removeActionHandler(surfaceId: surfaceId)
        }
    }

    private func handleInteraction(_ event: A2UIUserActionEvent) {
        a2uiAgent.handleUserAction(
            surfaceId: surfaceId,
            store: store,
            template: PrivacySurface.buildMessages(surfaceId: surfaceId),
            context: "analyticsOptIn=\(appModel.analyticsOptIn)",
            event: event
        )
    }
}

#Preview {
    SettingsView()
        .environmentObject(AppModel.preview())
        .environmentObject(A2UIAgent(mode: .preview))
}
