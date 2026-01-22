//
//  ContentView.swift
//  PruneApp
//
//  Created by Johan Sellström on 2026-01-17.
//

import AppKit
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
                return
            }
            await Task.yield()
            try? await Task.sleep(nanoseconds: 80_000_000)
        }
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
        Text("Status: \(appModel.statusLabel)")
        if let message = appModel.statusMessage {
            Text(message)
        }
        Divider()
        Button("Start") {
            appModel.startServices()
        }
        .disabled(!appModel.canStart)
        Button("Stop") {
            appModel.stopServices()
        }
        .disabled(!appModel.canStop)
        Divider()
        SettingsLink {
            Text("Open Dashboard")
        }
        .simultaneousGesture(TapGesture().onEnded {
            openSettings(tab: .setup)
        })
        Button("View Logs") {
            appModel.openLogs()
        }
        SettingsLink {
            Text("Help")
        }
        .simultaneousGesture(TapGesture().onEnded {
            openSettings(tab: .help)
        })
        Divider()
        Button("Quit") {
            NSApplication.shared.terminate(nil)
        }
    }
}

struct SettingsView: View {
    @EnvironmentObject private var appModel: AppModel

    var body: some View {
        TabView(selection: $appModel.selectedTab) {
            SetupView()
                .tabItem {
                    Label("Setup", systemImage: "wrench.and.screwdriver")
                }
                .tag(SettingsTab.setup)
            InceptionView()
                .tabItem {
                    Label("Inception", systemImage: "sparkles.rectangle.stack")
                }
                .tag(SettingsTab.inception)
            ServicesView()
                .tabItem {
                    Label("Services", systemImage: "bolt.horizontal.circle")
                }
                .tag(SettingsTab.services)
            IntegrationsView()
                .tabItem {
                    Label("Integrations", systemImage: "link")
                }
                .tag(SettingsTab.integrations)
            A2UIDiagnosticsView()
                .tabItem {
                    Label("A2UI", systemImage: "sparkles")
                }
                .tag(SettingsTab.a2ui)
            HelpView()
                .tabItem {
                    Label("Help", systemImage: "questionmark.circle")
                }
                .tag(SettingsTab.help)
            PrivacyView()
                .tabItem {
                    Label("Privacy", systemImage: "hand.raised")
                }
                .tag(SettingsTab.privacy)
        }
        .padding(20)
        .frame(minWidth: 760, minHeight: 540)
    }
}

struct SetupView: View {
    @EnvironmentObject private var appModel: AppModel
    @StateObject private var store = NormalizedSurfaceStore()
    private let surfaceId = "prune_setup"

    var body: some View {
        ScrollView {
            A2UISurfaceView(
                store: store,
                surfaceId: surfaceId,
                bindingProvider: SetupBindingProvider(appModel: appModel)
            )
            .padding()
        }
        .onAppear {
            store.reset()
            store.apply(SetupSurface.buildMessages(surfaceId: surfaceId))
        }
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

    private static func binding(_ key: String) -> JSONValue {
        .object(["binding": .string(key)])
    }

    private static func children(_ ids: [String]) -> JSONValue {
        .array(ids.map { .string($0) })
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

struct InceptionView: View {
    @EnvironmentObject private var appModel: AppModel

    @State private var template: InceptionTemplate = .web
    @State private var cliSubtype: Bool = false
    @State private var showInterview: Bool = false
    @State private var lastOutput: String = ""
    @State private var lastError: String? = nil

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                Text("Inception")
                    .font(.headline)

                if let repoFullName = appModel.normalizedRepoFullName() {
                    let mirror = appModel.paths.mirrorDirectory(repoFullName: repoFullName)

                    Group {
                        GroupBox {
                            VStack(alignment: .leading, spacing: 8) {
                                Text("Workspace")
                                    .font(.subheadline)
                                    .foregroundStyle(.secondary)

                                Text(repoFullName)
                                    .font(.body)

                                Text(mirror.path)
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                                    .textSelection(.enabled)

                                HStack {
                                    Button("Open Mirror") {
                                        NSWorkspace.shared.open(mirror)
                                    }

                                    Spacer()

                                    Button("Start A2UI Interview") {
                                        lastError = nil
                                        showInterview = true
                                    }
                                    .buttonStyle(.borderedProminent)
                                }
                            }
                        }

                        GroupBox {
                            VStack(alignment: .leading, spacing: 12) {
                                Text("Template")
                                    .font(.subheadline)
                                    .foregroundStyle(.secondary)

                                Picker("Project template", selection: $template) {
                                    ForEach(InceptionTemplate.allCases) { t in
                                        Text(t.title).tag(t)
                                    }
                                }
                                .pickerStyle(.menu)

                                Toggle("Treat this as a CLI-style repo", isOn: $cliSubtype)
                                    .help("Enables the 'cli' subtype in Prune preferences and bootstraps CLI-focused docs.")

                                Text("This selects the default Prune onboarding, golden paths, and strategy kit. You can refine everything in the interview before bootstrapping.")
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                            }
                        }

                        if let err = lastError {
                            Text(err)
                                .font(.caption)
                                .foregroundStyle(.red)
                                .textSelection(.enabled)
                        }

                        if !lastOutput.isEmpty {
                            GroupBox("Last output") {
                                ScrollView {
                                    Text(lastOutput)
                                        .font(.system(.caption, design: .monospaced))
                                        .textSelection(.enabled)
                                        .frame(maxWidth: .infinity, alignment: .leading)
                                }
                                .frame(maxHeight: 240)
                            }
                        }
                    }
                    // Sheet lives here so it has access to the same appModel + state.
                    .sheet(isPresented: $showInterview) {
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
                    }
                } else {
                    GroupBox {
                        VStack(alignment: .leading, spacing: 8) {
                            Text("No workspace configured")
                                .font(.headline)
                            Text("Set a GitHub repo in Setup first. Prune will create a local mirror and run inception/bootstrap inside that workspace.")
                                .font(.body)
                                .foregroundStyle(.secondary)
                        }
                    }
                }
            }
            .padding()
        }
    }
}

private struct InceptionInterviewSheet: View {
    @EnvironmentObject private var appModel: AppModel

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
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                VStack(alignment: .leading, spacing: 4) {
                    Text("A2UI Inception Interview")
                        .font(.headline)
                    Text("Edit preferences, optionally generate extra questions using the local Apple Foundation Model, then save + bootstrap.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                Spacer()
                Button("Close") { dismiss() }
            }

            Divider()

            ScrollView {
                A2UISurfaceView(
                    store: store,
                    surfaceId: surfaceId,
                    bindingProvider: InceptionBindingProvider { addManualOverride() }
                )
                    .frame(maxWidth: .infinity, alignment: .leading)
            }

            Divider()

            HStack(spacing: 10) {
                Button("Generate followups") {
                    Task { await generateFollowups() }
                }

                Spacer()

                Button("Save preferences") {
                    Task { await savePreferences() }
                }
                .buttonStyle(.bordered)

                Button("Save + bootstrap") {
                    Task { await saveAndBootstrap() }
                }
                .buttonStyle(.borderedProminent)
                .disabled(isBusy)
            }

            if !status.isEmpty {
                Text(status)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .textSelection(.enabled)
            }
        }
        .padding()
        .onAppear {
            store.reset()
            let msgs = buildInitialMsgs()
            for m in msgs { store.apply(m) }
        }
    }

    private func buildInitialMsgs() -> [NormalizedMsg] {
        let initialModel = defaultPreferencesDataModel(template: template, cliSubtype: cliSubtype)

        let components: [NormalizedComponent] = [
            NormalizedComponent(
                id: "root",
                type: "Column",
                props: [
                    "children": .array([
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
                        .string("overrides_add_button")
                    ])
                ]
            ),

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

    init(addOverride: @escaping () -> Void) {
        self.addOverride = addOverride
    }

    func perform(action: String) {
        switch action {
        case "add_override":
            addOverride()
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

@MainActor
private struct A2UISurfaceView: View {
    @ObservedObject var store: NormalizedSurfaceStore
    let surfaceId: String
    let bindingProvider: (any A2UIBindingProvider)?

    init(
        store: NormalizedSurfaceStore,
        surfaceId: String,
        bindingProvider: (any A2UIBindingProvider)? = nil
    ) {
        self.store = store
        self.surfaceId = surfaceId
        self.bindingProvider = bindingProvider
    }

    var body: some View {
        if let root = store.rootComponentId(for: surfaceId) {
            render(componentId: root)
        } else {
            Text("No surface")
                .font(.caption)
                .foregroundStyle(.secondary)
        }
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
                VStack(alignment: .leading, spacing: 12) {
                    ForEach(children, id: \.self) { cid in
                        render(componentId: cid)
                    }
                }
            )

        case "Row":
            let children = (component.props["children"]?.arrayValue ?? []).compactMap { $0.stringValue }
            return AnyView(
                HStack(alignment: .top, spacing: 12) {
                    ForEach(children, id: \.self) { cid in
                        render(componentId: cid)
                    }
                }
            )

        case "Divider":
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
            if selectable {
                return AnyView(view.textSelection(.enabled))
            }
            return AnyView(view)

        case "TextField":
            let label = resolved["label"]?.stringValue ?? ""
            let isMultiline = (component.props["multiline"]?.boolValue) ?? false
            let bindingKey = bindingKey(from: rawProps["value"])
            let path = (component.props["value"]?.objectValue?["path"]?.stringValue) ?? ""
            let style = resolved["style"]?.stringValue ?? rawProps["style"]?.stringValue

            if let bindingKey, let binding = bindingProvider?.stringBinding(for: bindingKey) {
                let field = TextField(label, text: binding)
                return applyTextFieldStyle(field, style: style)
            }

            if isMultiline {
                return AnyView(
                    VStack(alignment: .leading, spacing: 6) {
                        if !label.isEmpty { Text(label).font(.caption).foregroundStyle(.secondary) }
                        TextEditor(text: Binding(
                            get: { store.dataModelValue(surfaceId: surfaceId, path: path)?.stringValue ?? "" },
                            set: { store.setDataModelValue(surfaceId: surfaceId, path: path, value: .string($0)) }
                        ))
                        .frame(minHeight: 72)
                        .overlay(
                            RoundedRectangle(cornerRadius: 6)
                                .strokeBorder(Color.secondary.opacity(0.25), lineWidth: 1)
                        )
                    }
                )
            }
            let field = TextField(label, text: Binding(
                get: { store.dataModelValue(surfaceId: surfaceId, path: path)?.stringValue ?? "" },
                set: { store.setDataModelValue(surfaceId: surfaceId, path: path, value: .string($0)) }
            ))
            return applyTextFieldStyle(field, style: style)

        case "Toggle":
            let label = resolved["label"]?.stringValue ?? ""
            let bindingKey = bindingKey(from: rawProps["value"])
            let path = (component.props["value"]?.objectValue?["path"]?.stringValue) ?? ""
            if let bindingKey, let binding = bindingProvider?.boolBinding(for: bindingKey) {
                return AnyView(Toggle(label, isOn: binding))
            }
            return AnyView(
                Toggle(label, isOn: Binding(
                    get: { store.dataModelValue(surfaceId: surfaceId, path: path)?.boolValue ?? false },
                    set: { store.setDataModelValue(surfaceId: surfaceId, path: path, value: .bool($0)) }
                ))
            )

        case "Select":
            let label = resolved["label"]?.stringValue ?? ""
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
                return AnyView(
                    Picker(label, selection: binding) {
                        ForEach(options, id: \.1) { (lbl, val) in
                            Text(lbl).tag(val)
                        }
                    }
                    .pickerStyle(.menu)
                )
            }

            return AnyView(
                Picker(label, selection: Binding(
                    get: { store.dataModelValue(surfaceId: surfaceId, path: path)?.stringValue ?? "" },
                    set: { store.setDataModelValue(surfaceId: surfaceId, path: path, value: .string($0)) }
                )) {
                    ForEach(options, id: \.1) { (lbl, val) in
                        Text(lbl).tag(val)
                    }
                }
                .pickerStyle(.menu)
            )

        case "NumberField":
            let label = resolved["label"]?.stringValue ?? ""
            let bindingKey = bindingKey(from: rawProps["value"])
            let style = resolved["style"]?.stringValue ?? rawProps["style"]?.stringValue
            if let bindingKey, let binding = bindingProvider?.intBinding(for: bindingKey) {
                let field = TextField(label, value: binding, format: .number)
                return applyTextFieldStyle(field, style: style)
            }
            return AnyView(EmptyView())

        case "Button":
            let label = resolved["label"]?.stringValue ?? rawProps["label"]?.stringValue ?? "Action"
            let actionId = resolved["action"]?.stringValue ?? rawProps["action"]?.stringValue ?? ""
            let variant = resolved["variant"]?.stringValue ?? rawProps["variant"]?.stringValue
            let disabled = resolveBool(from: rawProps["disabled"], fallback: resolved["disabled"]) ?? false
            let button = Button(label) {
                bindingProvider?.perform(action: actionId)
            }
            .disabled(disabled)
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

    private func applyTextFieldStyle<V: View>(_ view: V, style: String?) -> AnyView {
        if style == "rounded" {
            return AnyView(view.textFieldStyle(.roundedBorder))
        }
        return AnyView(view)
    }
}

struct ServicesView: View {
    @EnvironmentObject private var appModel: AppModel

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                HStack(spacing: 12) {
                    Button("Start") {
                        appModel.startServices()
                    }
                    .disabled(!appModel.canStart)
                    Button("Stop") {
                        appModel.stopServices()
                    }
                    .disabled(!appModel.canStop)
                    Button("Refresh Status") {
                        appModel.refreshStatus()
                    }
                    .disabled(appModel.installState != .installed)
                }

                GroupBox("LaunchAgents") {
                    VStack(alignment: .leading, spacing: 8) {
                        Toggle("Manage services with LaunchAgents", isOn: appModel.binding(\.useLaunchAgents))
                        HStack {
                            Button("Install LaunchAgents") {
                                appModel.installLaunchAgents()
                            }
                            Button("Remove LaunchAgents") {
                                appModel.removeLaunchAgents()
                            }
                            Button("Open LaunchAgents Folder") {
                                appModel.openLaunchAgentsFolder()
                            }
                        }
                    }
                }

                ForEach(ServiceKind.allCases) { service in
                    ServiceRow(
                        name: service.displayName,
                        status: appModel.serviceStatus(for: service)
                    )
                }

                GroupBox("Service Arguments") {
                    VStack(alignment: .leading, spacing: 12) {
                        ArgumentsEditor(
                            title: "Tunnel",
                            text: appModel.argumentsBinding(for: .tunnel)
                        )
                        ArgumentsEditor(
                            title: "Sync",
                            text: appModel.argumentsBinding(for: .sync)
                        )
                        ArgumentsEditor(
                            title: "MCP",
                            text: appModel.argumentsBinding(for: .mcp)
                        )
                        Text("Arguments are optional and appended to defaults. One argument per line.")
                            .foregroundStyle(.secondary)
                    }
                }

                GroupBox("Logs") {
                    VStack(alignment: .leading, spacing: 8) {
                        HStack {
                            Button("View Logs") {
                                appModel.openLogs()
                            }
                            Button("Refresh Logs") {
                                appModel.refreshLogPreview()
                            }
                        }
                        TextEditor(text: $appModel.logPreview)
                            .font(.system(.body, design: .monospaced))
                            .frame(minHeight: 180)
                            .disabled(true)
                    }
                }

                StatusBanner(
                    message: appModel.statusMessage,
                    error: appModel.lastErrorMessage
                )
            }
        }
    }
}

struct IntegrationsView: View {
    @EnvironmentObject private var appModel: AppModel

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                GroupBox("Lovable MCP") {
                    VStack(alignment: .leading, spacing: 8) {
                        LabeledContent("Tunnel Base URL") {
                            TextField("https://example.tunnel.app", text: appModel.binding(\.tunnelBaseURL))
                                .textFieldStyle(.roundedBorder)
                        }
                        LabeledContent("MCP Server URL") {
                            TextField("MCP Server URL", text: appModel.mcpServerURLBinding)
                                .textFieldStyle(.roundedBorder)
                                .disabled(true)
                        }
                        LabeledContent("Webhook URL") {
                            TextField("Webhook URL", text: appModel.webhookURLBinding)
                                .textFieldStyle(.roundedBorder)
                                .disabled(true)
                        }
                        HStack {
                            Button("Copy MCP URL") {
                                appModel.copyMcpURL()
                            }
                            Button("Copy Webhook URL") {
                                appModel.copyWebhookURL()
                            }
                            Button("Test MCP Connection") {
                                appModel.testMcpConnection()
                            }
                        }
                        if let status = appModel.mcpTestStatus {
                            Text(status)
                        }
                    }
                }

                GroupBox("Lovable Instructions") {
                    VStack(alignment: .leading, spacing: 8) {
                        TextEditor(text: appModel.lovableInstructionsBinding)
                            .font(.system(.body, design: .monospaced))
                            .frame(minHeight: 160)
                            .disabled(true)
                        Button("Copy Instructions") {
                            appModel.copyLovableInstructions()
                        }
                    }
                }

                GroupBox("GitHub") {
                    VStack(alignment: .leading, spacing: 8) {
                        Text("Repository: \(appModel.config.repoFullName.isEmpty ? "Not set" : appModel.config.repoFullName)")
                        Text("Branch: \(appModel.config.defaultBranch)")
                        HStack {
                            Button("Create Webhook") {
                                appModel.createGitHubWebhook()
                            }
                            Button("Refresh Webhooks") {
                                appModel.refreshGitHubWebhooks()
                            }
                        }
                        if !appModel.webhooks.isEmpty {
                            VStack(alignment: .leading, spacing: 4) {
                                ForEach(appModel.webhooks) { hook in
                                    HStack {
                                        Text("Hook \(hook.id) - \(hook.displayURL)")
                                        Spacer()
                                        Button("Delete") {
                                            appModel.deleteGitHubWebhook(id: hook.id)
                                        }
                                    }
                                }
                            }
                        }
                        if let status = appModel.githubStatusMessage {
                            Text(status)
                        }
                    }
                }

                GroupBox("Secrets") {
                    VStack(alignment: .leading, spacing: 12) {
                        SecureField("GitHub Token", text: $appModel.githubTokenInput)
                            .textFieldStyle(.roundedBorder)
                        Button("Save GitHub Token") {
                            appModel.saveGitHubToken()
                        }
                        SecureField("Webhook Secret", text: $appModel.webhookSecretInput)
                            .textFieldStyle(.roundedBorder)
                        Button("Save Webhook Secret") {
                            appModel.saveWebhookSecret()
                        }
                        SecureField("MCP Bearer Token (optional)", text: $appModel.mcpTokenInput)
                            .textFieldStyle(.roundedBorder)
                        Button("Save MCP Token") {
                            appModel.saveMcpToken()
                        }
                    }
                }

                StatusBanner(
                    message: appModel.statusMessage,
                    error: appModel.lastErrorMessage
                )
            }
        }
    }
}

struct A2UIDiagnosticsView: View {
    private struct A2UIFixtureReport: Equatable {
        let surfaceId: String
        let protocolVersion: String?
        let rootComponentId: String?
        let componentCount: Int
        let resolvedText: String?
        let dataModelJSON: String
        let errors: [String]
    }

    private enum InputMode: String, CaseIterable {
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

    private enum FixtureVersion: String, CaseIterable {
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

    @State private var inputMode: InputMode = .fixture
    @State private var fixtureVersion: FixtureVersion = .v09
    @State private var filePath: String = ""
    @State private var isFileImporterPresented = false
    @State private var streamURLText: String = "http://localhost:47802/a2ui"
    @State private var statusMessage: String?
    @State private var errorMessage: String?
    @State private var report: A2UIFixtureReport?
    @State private var errors: [String] = []
    @State private var isRunning = false
    @State private var isStreaming = false
    @State private var streamTask: Task<Void, Never>?
    @State private var store = NormalizedSurfaceStore()

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                Text("A2UI Inception Diagnostics")
                    .font(.title2)
                Text("Run fixtures, load JSONL, or stream live messages into A2UIRuntime.")
                    .foregroundStyle(.secondary)
                inputSection
                statusSection
                outputSection
            }
            .frame(maxWidth: .infinity, alignment: .leading)
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
                }
                filePath = url.path
                loadJSONL(from: url)
            case .failure(let error):
                errorMessage = "File import failed: \(error.localizedDescription)"
            }
        }
        .onAppear {
            guard report == nil, !isRunning, !isStreaming else { return }
            DispatchQueue.main.async {
                runFixture()
            }
        }
        .onDisappear {
            stopStreaming()
        }
    }

    @ViewBuilder
    private var inputSection: some View {
        GroupBox("Input") {
            VStack(alignment: .leading, spacing: 12) {
                Picker("Mode", selection: $inputMode) {
                    ForEach(InputMode.allCases, id: \.self) { mode in
                        Text(mode.label).tag(mode)
                    }
                }
                .pickerStyle(.segmented)
                modeControls
            }
        }
    }

    @ViewBuilder
    private var modeControls: some View {
        switch inputMode {
        case .fixture:
            Picker("Fixture Version", selection: $fixtureVersion) {
                ForEach(FixtureVersion.allCases, id: \.self) { version in
                    Text(version.label).tag(version)
                }
            }
            .pickerStyle(.segmented)
            HStack(spacing: 12) {
                Button("Run Fixture") {
                    runFixture()
                }
                Button("Reset") {
                    resetState()
                }
                if isRunning {
                    ProgressView()
                }
            }
        case .file:
            TextField("JSONL file path", text: $filePath)
                .textFieldStyle(.roundedBorder)
            HStack(spacing: 12) {
                Button("Browse") {
                    isFileImporterPresented = true
                }
                Button("Load File") {
                    loadJSONLFromPath()
                }
                Button("Reset") {
                    resetState()
                }
                if isRunning {
                    ProgressView()
                }
            }
            Text("Each line should be a JSON object (A2UI message envelope).")
                .font(.caption)
                .foregroundStyle(.secondary)
        case .live:
            TextField("Stream URL", text: $streamURLText)
                .textFieldStyle(.roundedBorder)
            HStack(spacing: 12) {
                if isStreaming {
                    Button("Stop Stream") {
                        stopStreaming()
                    }
                } else {
                    Button("Start Stream") {
                        startStreaming()
                    }
                }
                Button("Reset") {
                    resetState()
                }
                if isStreaming {
                    ProgressView()
                }
            }
            Text("Stream expects newline-delimited JSON objects over HTTP.")
                .font(.caption)
                .foregroundStyle(.secondary)
        }
    }

    @ViewBuilder
    private var statusSection: some View {
        VStack(alignment: .leading, spacing: 6) {
            if let statusMessage {
                Text(statusMessage)
                    .foregroundStyle(.secondary)
            }
            if let errorMessage {
                Text(errorMessage)
                    .foregroundStyle(.red)
            }
        }
    }

    @ViewBuilder
    private var outputSection: some View {
        if let report {
            GroupBox("Surface") {
                VStack(alignment: .leading, spacing: 6) {
                    Text("Surface ID: \(report.surfaceId)")
                    Text("Protocol: \(report.protocolVersion ?? "unknown")")
                    Text("Root component: \(report.rootComponentId ?? "unknown")")
                    Text("Components: \(report.componentCount)")
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            GroupBox("Resolved Text") {
                Text(report.resolvedText ?? "No bound text resolved.")
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
            GroupBox("Data Model") {
                Text(report.dataModelJSON)
                    .font(.system(.body, design: .monospaced))
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
            if !report.errors.isEmpty {
                GroupBox("Adapter Errors") {
                    VStack(alignment: .leading, spacing: 6) {
                        ForEach(report.errors, id: \.self) { error in
                            Text(error)
                                .foregroundStyle(.red)
                        }
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
            }
        } else {
            Text("No A2UI output yet.")
                .foregroundStyle(.secondary)
        }
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
                return
            }
            runLines(
                lines,
                label: "Loaded \(lines.count) lines from \(url.lastPathComponent).",
                preferredVersion: nil
            )
        } catch {
            errorMessage = "Failed to read file: \(error.localizedDescription)"
        }
    }

    @MainActor
    private func startStreaming() {
        stopStreaming()
        guard let url = URL(string: streamURLText), url.scheme != nil else {
            errorMessage = "Invalid stream URL."
            return
        }

        resetState()
        isStreaming = true
        statusMessage = "Connecting to stream..."

        let adapter = A2UIProtocolAdapter(enableV09: true)
        streamTask = Task { @MainActor in
            do {
                let (bytes, _) = try await URLSession.shared.bytes(from: url)
                statusMessage = "Streaming..."
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
        }
    }

    @MainActor
    private func stopStreaming() {
        streamTask?.cancel()
        streamTask = nil
        if isStreaming {
            isStreaming = false
            statusMessage = "Stream stopped."
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

        let adapter = A2UIProtocolAdapter(enableV09: true, preferredVersion: preferredVersion)
        for line in lines {
            let trimmed = line.trimmingCharacters(in: .whitespacesAndNewlines)
            guard !trimmed.isEmpty else { continue }
            let messages = adapter.decode(line: trimmed)
            applyMessages(messages)
        }

        isRunning = false
    }

    @MainActor
    private func applyMessages(_ messages: [NormalizedMsg]) {
        for message in messages {
            switch message {
            case .error(let message):
                errors.append(message)
            default:
                store.apply(message)
            }
        }
        updateReport()
    }

    @MainActor
    private func updateReport() {
        guard let surfaceId = store.surfaces.keys.sorted().first,
              let surface = store.surfaces[surfaceId] else {
            report = nil
            return
        }

        let rootId = store.rootComponentId(for: surfaceId)
        var resolvedText: String?
        if let rootId,
           let props = store.resolvedProps(surfaceId: surfaceId, componentId: rootId),
           let textValue = props["text"] {
            resolvedText = textValue.stringValue ?? prettyJSON(textValue)
        }

        report = A2UIFixtureReport(
            surfaceId: surfaceId,
            protocolVersion: surface.info.protocolVersion?.rawValue,
            rootComponentId: rootId,
            componentCount: surface.components.count,
            resolvedText: resolvedText,
            dataModelJSON: prettyJSON(surface.dataModel),
            errors: errors
        )
    }

    @MainActor
    private func resetState() {
        store = NormalizedSurfaceStore()
        errors = []
        report = nil
        errorMessage = nil
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
}

struct HelpView: View {
    @EnvironmentObject private var appModel: AppModel

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                if case let .missing(reason) = appModel.gitAvailability {
                    GroupBox("Git Required") {
                        VStack(alignment: .leading, spacing: 8) {
                            Text(reason)
                                .foregroundStyle(.primary)
                            HStack(spacing: 12) {
                                Button("Install Command Line Tools") {
                                    appModel.installCommandLineTools()
                                }
                                Button("Recheck") {
                                    appModel.refreshGitAvailability()
                                }
                            }
                        }
                    }
                }

                GroupBox("Prerequisites") {
                    VStack(alignment: .leading, spacing: 6) {
                        Text("macOS 13 or newer.")
                        Text("Git installed (Xcode Command Line Tools). Required to mirror repos and detect remotes.")
                        Text("If Git is missing, run: xcode-select --install")
                        Text("Network access for tunnel + webhooks.")
                        Text("Optional: GitHub token + webhook secret for GitHub sync.")
                    }
                }

                GroupBox("Quickstart") {
                    VStack(alignment: .leading, spacing: 6) {
                        Text("1. Install Prune.app (Install.command); bundled dependencies install on first launch.")
                        Text("2. Click Start to bring up the tunnel and services.")
                        Text("3. Copy the MCP Server URL.")
                        Text("4. In Lovable: Settings -> Connectors -> Personal connectors -> New MCP server.")
                        Text("5. Connect Lovable project to GitHub (default branch sync).")
                        Text("6. Confirm Last Indexed SHA updates after edits.")
                    }
                }

                GroupBox("Troubleshooting") {
                    VStack(alignment: .leading, spacing: 6) {
                        Text("MCP not reachable: confirm tunnel is running and URL is copied exactly.")
                        Text("Webhook failing: ensure GitHub token and webhook secret are stored.")
                        Text("Index not updating: check sync service status and logs.")
                        Text("Tunnel expired: stop and start to refresh.")
                    }
                }

                GroupBox("Diagnostics") {
                    VStack(alignment: .leading, spacing: 8) {
                        Button("Export Diagnostics") {
                            appModel.exportDiagnostics()
                        }
                        if let path = appModel.lastDiagnosticsPath {
                            Text("Last export: \(path.path)")
                        }
                    }
                }

                StatusBanner(
                    message: appModel.statusMessage,
                    error: appModel.lastErrorMessage
                )
            }
        }
        .onAppear {
            appModel.refreshGitAvailability()
        }
    }
}

struct PrivacyView: View {
    @EnvironmentObject private var appModel: AppModel

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Toggle("Share anonymous usage analytics", isOn: $appModel.analyticsOptIn)
            Text("Analytics are opt-in and never include code, prompts, or repository contents.")
                .foregroundStyle(.secondary)
            Link("Privacy Policy", destination: URL(string: "https://prune.dev/privacy")!)
            Spacer()
        }
    }
}

struct StatusBadge: View {
    let title: String
    let value: String
    let tone: StatusTone

    var body: some View {
        HStack(spacing: 8) {
            Circle()
                .fill(tone.color)
                .frame(width: 10, height: 10)
            VStack(alignment: .leading, spacing: 2) {
                Text(title)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Text(value)
                    .font(.headline)
            }
        }
        .padding(10)
        .background(.thinMaterial, in: RoundedRectangle(cornerRadius: 8))
    }
}

struct PathRow: View {
    let title: String
    let path: String
    let onCopy: () -> Void

    var body: some View {
        HStack {
            VStack(alignment: .leading, spacing: 2) {
                Text(title)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Text(path)
                    .font(.system(.body, design: .monospaced))
            }
            Spacer()
            Button("Copy") {
                onCopy()
            }
        }
    }
}

struct ArgumentsEditor: View {
    let title: String
    @Binding var text: String

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(title)
                .font(.caption)
                .foregroundStyle(.secondary)
            TextEditor(text: $text)
                .font(.system(.body, design: .monospaced))
                .frame(minHeight: 70)
                .overlay(
                    RoundedRectangle(cornerRadius: 6)
                        .stroke(Color.secondary.opacity(0.2))
                )
        }
    }
}

struct ServiceRow: View {
    let name: String
    let status: ServiceStatus

    var body: some View {
        HStack {
            Text(name)
                .font(.headline)
            Spacer()
            StatusPill(text: status.state.label, tone: status.state.tone)
        }
        if !status.detail.isEmpty {
            Text(status.detail)
                .foregroundStyle(.secondary)
        }
    }
}

struct StatusPill: View {
    let text: String
    let tone: StatusTone

    var body: some View {
        Text(text)
            .font(.caption)
            .padding(.horizontal, 8)
            .padding(.vertical, 4)
            .background(tone.color.opacity(0.15), in: Capsule())
            .foregroundStyle(tone.color)
    }
}

struct StatusBanner: View {
    let message: String?
    let error: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            if let message {
                Text(message)
                    .foregroundStyle(.secondary)
            }
            if let error {
                Text(error)
                    .foregroundStyle(.red)
            }
        }
    }
}

#Preview {
    SettingsView()
        .environmentObject(AppModel.preview())
}
