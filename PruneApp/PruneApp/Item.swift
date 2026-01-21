//
//  Item.swift
//  PruneApp
//
//  Created by Johan Sellström on 2026-01-17.
//

import AppKit
import Combine
import Darwin
import Foundation
import Security
import SwiftUI

enum SettingsTab: Hashable {
    case setup
    case inception
    case services
    case integrations
    case a2ui
    case help
    case privacy
}

enum StatusTone {
    case neutral
    case good
    case warning
    case bad

    var color: Color {
        switch self {
        case .neutral:
            return .gray
        case .good:
            return .green
        case .warning:
            return .orange
        case .bad:
            return .red
        }
    }
}

enum AppStatus {
    case stopped
    case starting
    case running
    case stopping
    case error

    var label: String {
        switch self {
        case .stopped:
            return "Stopped"
        case .starting:
            return "Starting"
        case .running:
            return "Running"
        case .stopping:
            return "Stopping"
        case .error:
            return "Error"
        }
    }

    var symbolName: String {
        switch self {
        case .stopped:
            return "circle"
        case .starting:
            return "arrow.triangle.2.circlepath"
        case .running:
            return "checkmark.circle.fill"
        case .stopping:
            return "pause.circle"
        case .error:
            return "exclamationmark.triangle.fill"
        }
    }
}

enum InstallState {
    case notInstalled
    case installing
    case installed
    case failed

    var label: String {
        switch self {
        case .notInstalled:
            return "Not installed"
        case .installing:
            return "Installing"
        case .installed:
            return "Installed"
        case .failed:
            return "Failed"
        }
    }

    var tone: StatusTone {
        switch self {
        case .notInstalled:
            return .neutral
        case .installing:
            return .warning
        case .installed:
            return .good
        case .failed:
            return .bad
        }
    }
}

enum ToolAvailability: Equatable {
    case unknown
    case available
    case missing(String)
}

enum ServiceState {
    case stopped
    case starting
    case running
    case stopping
    case failed

    var label: String {
        switch self {
        case .stopped:
            return "Stopped"
        case .starting:
            return "Starting"
        case .running:
            return "Running"
        case .stopping:
            return "Stopping"
        case .failed:
            return "Failed"
        }
    }

    var tone: StatusTone {
        switch self {
        case .stopped:
            return .neutral
        case .starting:
            return .warning
        case .running:
            return .good
        case .stopping:
            return .warning
        case .failed:
            return .bad
        }
    }
}

struct ServiceStatus {
    var state: ServiceState
    var detail: String
}

enum ServiceKind: String, CaseIterable, Identifiable {
    case tunnel
    case sync
    case mcp

    var id: String { rawValue }

    var displayName: String {
        switch self {
        case .tunnel:
            return "Tunnel"
        case .sync:
            return "Sync"
        case .mcp:
            return "MCP"
        }
    }
}

struct BinaryNames: Codable {
    var mcp: String
    var sync: String
    var tunnel: String

    static let `default` = BinaryNames(
        mcp: "prune-mcp",
        sync: "prune-sync",
        tunnel: "cloudflared"
    )
}

struct ServiceArguments: Codable {
    var mcp: [String]
    var sync: [String]
    var tunnel: [String]

    static let `default` = ServiceArguments(
        mcp: [],
        sync: [],
        tunnel: []
    )
}

extension ServiceArguments {
    func args(for service: ServiceKind) -> [String] {
        switch service {
        case .tunnel:
            return tunnel
        case .sync:
            return sync
        case .mcp:
            return mcp
        }
    }

    mutating func setArgs(_ args: [String], for service: ServiceKind) {
        switch service {
        case .tunnel:
            tunnel = args
        case .sync:
            sync = args
        case .mcp:
            mcp = args
        }
    }
}

struct AppConfig: Codable {
    var repoFullName: String
    var defaultBranch: String
    var mcpPort: Int
    var webhookPort: Int
    var tunnelBaseURL: String
    var lastIndexedSha: String
    var webhookStatus: String
    var binaryNames: BinaryNames
    var serviceArguments: ServiceArguments
    var useLaunchAgents: Bool

    enum CodingKeys: String, CodingKey {
        case repoFullName
        case defaultBranch
        case mcpPort
        case webhookPort
        case tunnelBaseURL
        case lastIndexedSha
        case webhookStatus
        case binaryNames
        case serviceArguments
        case useLaunchAgents
    }

    init(
        repoFullName: String,
        defaultBranch: String,
        mcpPort: Int,
        webhookPort: Int,
        tunnelBaseURL: String,
        lastIndexedSha: String,
        webhookStatus: String,
        binaryNames: BinaryNames,
        serviceArguments: ServiceArguments,
        useLaunchAgents: Bool
    ) {
        self.repoFullName = repoFullName
        self.defaultBranch = defaultBranch
        self.mcpPort = mcpPort
        self.webhookPort = webhookPort
        self.tunnelBaseURL = tunnelBaseURL
        self.lastIndexedSha = lastIndexedSha
        self.webhookStatus = webhookStatus
        self.binaryNames = binaryNames
        self.serviceArguments = serviceArguments
        self.useLaunchAgents = useLaunchAgents
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        repoFullName = try container.decodeIfPresent(String.self, forKey: .repoFullName) ?? ""
        defaultBranch = try container.decodeIfPresent(String.self, forKey: .defaultBranch) ?? "main"
        mcpPort = try container.decodeIfPresent(Int.self, forKey: .mcpPort) ?? 47800
        webhookPort = try container.decodeIfPresent(Int.self, forKey: .webhookPort) ?? 47801
        tunnelBaseURL = try container.decodeIfPresent(String.self, forKey: .tunnelBaseURL) ?? ""
        lastIndexedSha = try container.decodeIfPresent(String.self, forKey: .lastIndexedSha) ?? ""
        webhookStatus = try container.decodeIfPresent(String.self, forKey: .webhookStatus) ?? "unknown"
        binaryNames = try container.decodeIfPresent(BinaryNames.self, forKey: .binaryNames) ?? .default
        serviceArguments = try container.decodeIfPresent(ServiceArguments.self, forKey: .serviceArguments) ?? .default
        useLaunchAgents = try container.decodeIfPresent(Bool.self, forKey: .useLaunchAgents) ?? false
    }

    func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        try container.encode(repoFullName, forKey: .repoFullName)
        try container.encode(defaultBranch, forKey: .defaultBranch)
        try container.encode(mcpPort, forKey: .mcpPort)
        try container.encode(webhookPort, forKey: .webhookPort)
        try container.encode(tunnelBaseURL, forKey: .tunnelBaseURL)
        try container.encode(lastIndexedSha, forKey: .lastIndexedSha)
        try container.encode(webhookStatus, forKey: .webhookStatus)
        try container.encode(binaryNames, forKey: .binaryNames)
        try container.encode(serviceArguments, forKey: .serviceArguments)
        try container.encode(useLaunchAgents, forKey: .useLaunchAgents)
    }

    static let `default` = AppConfig(
        repoFullName: "",
        defaultBranch: "main",
        mcpPort: 47800,
        webhookPort: 47801,
        tunnelBaseURL: "",
        lastIndexedSha: "",
        webhookStatus: "unknown",
        binaryNames: .default,
        serviceArguments: .default,
        useLaunchAgents: false
    )
}

struct AppPaths {
    let appSupport: URL
    let bin: URL
    let mirrors: URL
    let logs: URL
    let launchAgents: URL
    let configFile: URL
    let syncStatusFile: URL
    let logFile: URL

    static func defaultPaths() -> AppPaths {
        let fileManager = FileManager.default
        let appSupport = fileManager.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("Prune", isDirectory: true)
        let bin = appSupport.appendingPathComponent("bin", isDirectory: true)
        let mirrors = appSupport.appendingPathComponent("mirrors", isDirectory: true)
        let library = fileManager.urls(for: .libraryDirectory, in: .userDomainMask)[0]
        let logs = library.appendingPathComponent("Logs/Prune", isDirectory: true)
        let launchAgents = library.appendingPathComponent("LaunchAgents", isDirectory: true)
        let configFile = appSupport.appendingPathComponent("config.json")
        let syncStatusFile = appSupport.appendingPathComponent("sync-status.json")
        let logFile = logs.appendingPathComponent("prune.log")

        return AppPaths(
            appSupport: appSupport,
            bin: bin,
            mirrors: mirrors,
            logs: logs,
            launchAgents: launchAgents,
            configFile: configFile,
            syncStatusFile: syncStatusFile,
            logFile: logFile
        )
    }

    func mirrorDirectory(repoFullName: String) -> URL {
        let slug = repoFullName.replacingOccurrences(of: "/", with: "__")
        return mirrors.appendingPathComponent(slug, isDirectory: true)
    }

    func ceIndexDirectory(repoFullName: String) -> URL {
        mirrorDirectory(repoFullName: repoFullName)
            .appendingPathComponent(".ce", isDirectory: true)
    }

    func ceIndexDatabase(repoFullName: String) -> URL {
        ceIndexDirectory(repoFullName: repoFullName)
            .appendingPathComponent("index.sqlite")
    }

    func ceHnswDirectory(repoFullName: String) -> URL {
        ceIndexDirectory(repoFullName: repoFullName)
            .appendingPathComponent("hnsw", isDirectory: true)
    }

    func launchAgentPlist(label: String) -> URL {
        launchAgents.appendingPathComponent("\(label).plist")
    }
}

struct ConfigStore {
    let paths: AppPaths

    func load() throws -> AppConfig {
        let data = try Data(contentsOf: paths.configFile)
        return try JSONDecoder().decode(AppConfig.self, from: data)
    }

    func save(_ config: AppConfig) throws {
        try FileManager.default.createDirectory(
            at: paths.appSupport,
            withIntermediateDirectories: true,
            attributes: nil
        )
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        let data = try encoder.encode(config)
        try data.write(to: paths.configFile, options: [.atomic])
    }
}

struct LogStore {
    let paths: AppPaths

    func ensureLogFile() throws {
        let fileManager = FileManager.default
        try fileManager.createDirectory(at: paths.logs, withIntermediateDirectories: true, attributes: nil)
        if !fileManager.fileExists(atPath: paths.logFile.path) {
            fileManager.createFile(atPath: paths.logFile.path, contents: nil)
        }
    }

    func openForAppending() -> FileHandle? {
        do {
            try ensureLogFile()
            let handle = try FileHandle(forWritingTo: paths.logFile)
            try handle.seekToEnd()
            return handle
        } catch {
            return nil
        }
    }

    func append(_ message: String) {
        let formatter = DateFormatter()
        formatter.dateFormat = "yyyy-MM-dd HH:mm:ss"
        let line = "[\(formatter.string(from: Date()))] \(message)\n"

        do {
            try ensureLogFile()
            let handle = try FileHandle(forWritingTo: paths.logFile)
            try handle.seekToEnd()
            if let data = line.data(using: .utf8) {
                handle.write(data)
            }
            try handle.close()
        } catch {
            // Best effort logging.
        }
    }

    func readTail(maxBytes: Int) -> String {
        guard let handle = try? FileHandle(forReadingFrom: paths.logFile) else {
            return ""
        }

        do {
            let data = try handle.readToEnd() ?? Data()
            try handle.close()
            let tail = data.suffix(maxBytes)
            return String(data: tail, encoding: .utf8) ?? ""
        } catch {
            return ""
        }
    }
}

struct InstallResult {
    var warnings: [String] = []
}

struct BundledBinaryLocator {
    static func url(for name: String) -> URL? {
        let fileManager = FileManager.default
        if let bundleURL = Bundle.main.resourceURL {
            let binURL = bundleURL.appendingPathComponent("bin", isDirectory: true)
            let candidate = binURL.appendingPathComponent(name)
            if fileManager.fileExists(atPath: candidate.path) {
                return candidate
            }
        }
        return Bundle.main.url(forResource: name, withExtension: nil)
    }
}

struct SyncStatusFile: Codable {
    var lastIndexedSha: String?
    var lastIndexedAt: UInt64?
    var lastError: String?
    var lastEvent: String?

    enum CodingKeys: String, CodingKey {
        case lastIndexedSha = "last_indexed_sha"
        case lastIndexedAt = "last_indexed_at"
        case lastError = "last_error"
        case lastEvent = "last_event"
    }
}

struct Installer {
    static func detectInstalled(paths: AppPaths) -> Bool {
        FileManager.default.fileExists(atPath: paths.configFile.path) &&
            FileManager.default.fileExists(atPath: paths.bin.path)
    }

    static func install(
        paths: AppPaths,
        config: AppConfig,
        reinstallOnly: Bool,
        logStore: LogStore
    ) throws -> InstallResult {
        let fileManager = FileManager.default
        var result = InstallResult()

        if !reinstallOnly {
            try fileManager.createDirectory(at: paths.appSupport, withIntermediateDirectories: true, attributes: nil)
            try fileManager.createDirectory(at: paths.bin, withIntermediateDirectories: true, attributes: nil)
            try fileManager.createDirectory(at: paths.mirrors, withIntermediateDirectories: true, attributes: nil)
            try fileManager.createDirectory(at: paths.logs, withIntermediateDirectories: true, attributes: nil)

            if !config.repoFullName.isEmpty {
                try fileManager.createDirectory(
                    at: paths.mirrorDirectory(repoFullName: config.repoFullName),
                    withIntermediateDirectories: true,
                    attributes: nil
                )
                try fileManager.createDirectory(
                    at: paths.ceIndexDirectory(repoFullName: config.repoFullName),
                    withIntermediateDirectories: true,
                    attributes: nil
                )
                try fileManager.createDirectory(
                    at: paths.ceHnswDirectory(repoFullName: config.repoFullName),
                    withIntermediateDirectories: true,
                    attributes: nil
                )
            }
        }

        let binaries = Set([
            config.binaryNames.tunnel,
            config.binaryNames.sync,
            config.binaryNames.mcp,
            "ce",
            "ce-mcp"
        ])

        var missing: [String] = []
        var sources: [(name: String, url: URL)] = []

        for name in binaries {
            if name.hasPrefix("/") {
                continue
            }
            guard let resourceURL = BundledBinaryLocator.url(for: name) else {
                missing.append(name)
                continue
            }
            sources.append((name: name, url: resourceURL))
        }

        if !missing.isEmpty {
            let detail = missing.sorted().joined(separator: ", ")
            throw NSError(
                domain: "PruneInstaller",
                code: 1,
                userInfo: [NSLocalizedDescriptionKey: "Missing bundled binaries: \(detail)"]
            )
        }

        for entry in sources {
            let destination = paths.bin.appendingPathComponent(entry.name)
            if fileManager.fileExists(atPath: destination.path) {
                try fileManager.removeItem(at: destination)
            }
            try fileManager.copyItem(at: entry.url, to: destination)
            try fileManager.setAttributes([.posixPermissions: 0o755], ofItemAtPath: destination.path)
        }

        logStore.append("Install completed. Warnings: \(result.warnings.count)")
        return result
    }
}

struct DiagnosticsExporter {
    static func export(
        paths: AppPaths,
        config: AppConfig,
        serviceStatuses: [ServiceKind: ServiceStatus],
        logStore: LogStore
    ) throws -> URL {
        let fileManager = FileManager.default
        let downloads = fileManager.urls(for: .downloadsDirectory, in: .userDomainMask).first
            ?? fileManager.temporaryDirectory
        let formatter = DateFormatter()
        formatter.dateFormat = "yyyyMMdd-HHmmss"
        let folderURL = downloads.appendingPathComponent(
            "Prune-Diagnostics-\(formatter.string(from: Date()))",
            isDirectory: true
        )
        try fileManager.createDirectory(at: folderURL, withIntermediateDirectories: true, attributes: nil)

        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]

        let configData = try encoder.encode(config)
        try configData.write(to: folderURL.appendingPathComponent("config.json"), options: [.atomic])

        var statusDump: [String: String] = [:]
        for (service, status) in serviceStatuses {
            statusDump[service.rawValue] = "\(status.state.label): \(status.detail)"
        }
        let statusData = try encoder.encode(statusDump)
        try statusData.write(to: folderURL.appendingPathComponent("status.json"), options: [.atomic])

        if fileManager.fileExists(atPath: paths.logFile.path) {
            let logTarget = folderURL.appendingPathComponent("prune.log")
            if fileManager.fileExists(atPath: logTarget.path) {
                try fileManager.removeItem(at: logTarget)
            }
            try fileManager.copyItem(at: paths.logFile, to: logTarget)
        }

        if fileManager.fileExists(atPath: paths.syncStatusFile.path) {
            let statusTarget = folderURL.appendingPathComponent("sync-status.json")
            if fileManager.fileExists(atPath: statusTarget.path) {
                try fileManager.removeItem(at: statusTarget)
            }
            try fileManager.copyItem(at: paths.syncStatusFile, to: statusTarget)
        }

        let systemInfo: [String: String] = [
            "osVersion": ProcessInfo.processInfo.operatingSystemVersionString,
            "appVersion": Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? "unknown",
            "buildNumber": Bundle.main.infoDictionary?["CFBundleVersion"] as? String ?? "unknown"
        ]
        let systemData = try encoder.encode(systemInfo)
        try systemData.write(to: folderURL.appendingPathComponent("system.json"), options: [.atomic])

        return folderURL
    }
}

struct KeychainError: Error {
    let status: OSStatus
}

final class KeychainStore {
    private let service = "com.prune.app"

    func save(_ value: String, account: String) throws {
        let data = Data(value.utf8)
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account
        ]

        let attributes: [String: Any] = [
            kSecValueData as String: data
        ]

        let status = SecItemUpdate(query as CFDictionary, attributes as CFDictionary)
        if status == errSecItemNotFound {
            var newItem = query
            newItem[kSecValueData as String] = data
            let addStatus = SecItemAdd(newItem as CFDictionary, nil)
            if addStatus != errSecSuccess {
                throw KeychainError(status: addStatus)
            }
            return
        }

        if status != errSecSuccess {
            throw KeychainError(status: status)
        }
    }

    func read(account: String) -> String? {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
            kSecReturnData as String: true,
            kSecMatchLimit as String: kSecMatchLimitOne
        ]

        var item: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &item)
        if status == errSecSuccess, let data = item as? Data {
            return String(data: data, encoding: .utf8)
        }
        return nil
    }

    func delete(account: String) {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account
        ]
        SecItemDelete(query as CFDictionary)
    }
}

struct GitHubWebhook: Identifiable, Decodable {
    struct Config: Decodable {
        let url: String?
    }

    let id: Int
    let active: Bool?
    let config: Config?

    var displayURL: String {
        config?.url ?? "unknown"
    }
}

struct GitHubClient {
    let token: String

    func listWebhooks(repoFullName: String) async throws -> [GitHubWebhook] {
        let request = try makeRequest(
            path: "repos/\(repoFullName)/hooks",
            method: "GET",
            body: nil
        )
        let (data, _) = try await URLSession.shared.data(for: request)
        return try JSONDecoder().decode([GitHubWebhook].self, from: data)
    }

    func createWebhook(repoFullName: String, callbackURL: String, secret: String) async throws -> GitHubWebhook {
        let payload: [String: Any] = [
            "name": "web",
            "active": true,
            "events": ["push"],
            "config": [
                "url": callbackURL,
                "content_type": "json",
                "secret": secret,
                "insecure_ssl": "0"
            ]
        ]
        let body = try JSONSerialization.data(withJSONObject: payload, options: [])
        let request = try makeRequest(
            path: "repos/\(repoFullName)/hooks",
            method: "POST",
            body: body
        )
        let (data, _) = try await URLSession.shared.data(for: request)
        return try JSONDecoder().decode(GitHubWebhook.self, from: data)
    }

    func deleteWebhook(repoFullName: String, id: Int) async throws {
        let request = try makeRequest(
            path: "repos/\(repoFullName)/hooks/\(id)",
            method: "DELETE",
            body: nil
        )
        _ = try await URLSession.shared.data(for: request)
    }

    private func makeRequest(path: String, method: String, body: Data?) throws -> URLRequest {
        guard let url = URL(string: "https://api.github.com/\(path)") else {
            throw URLError(.badURL)
        }
        var request = URLRequest(url: url)
        request.httpMethod = method
        request.setValue("application/vnd.github+json", forHTTPHeaderField: "Accept")
        request.setValue("PruneApp", forHTTPHeaderField: "User-Agent")
        request.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
        if let body {
            request.httpBody = body
            request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        }
        return request
    }
}

@MainActor
final class AppModel: ObservableObject {
    private enum KeychainAccount {
        static let githubToken = "github-token"
        static let webhookSecret = "webhook-secret"
        static let mcpToken = "mcp-token"
    }

    let paths: AppPaths
    private let configStore: ConfigStore
    private let logStore: LogStore
    private let keychain = KeychainStore()
    private var processes: [ServiceKind: Process] = [:]
    private var logHandle: FileHandle?

    @Published var config: AppConfig
    @Published var installState: InstallState
    @Published var selectedTab: SettingsTab = .setup
    @Published var statusMessage: String?
    @Published var lastErrorMessage: String?
    @Published var serviceStatuses: [ServiceKind: ServiceStatus]
    @Published var analyticsOptIn: Bool {
        didSet {
            UserDefaults.standard.set(analyticsOptIn, forKey: "analyticsOptIn")
            logStore.append("Analytics opt-in set to \(analyticsOptIn)")
        }
    }
    @Published var mcpTestStatus: String?
    @Published var githubStatusMessage: String?
    @Published var webhooks: [GitHubWebhook] = []
    @Published var logPreview: String = ""
    @Published var lastDiagnosticsPath: URL?
    @Published var githubTokenInput: String = ""
    @Published var webhookSecretInput: String = ""
    @Published var mcpTokenInput: String = ""
    @Published var gitAvailability: ToolAvailability = .unknown

    init() {
        let paths = AppPaths.defaultPaths()
        let configStore = ConfigStore(paths: paths)
        let logStore = LogStore(paths: paths)
        var loadedConfig = (try? configStore.load()) ?? AppConfig.default
        if loadedConfig.binaryNames.mcp == "ce-mcp" {
            loadedConfig.binaryNames.mcp = "prune-mcp"
            try? configStore.save(loadedConfig)
        }

        self.paths = paths
        self.configStore = configStore
        self.logStore = logStore
        self.config = loadedConfig
        self.installState = Installer.detectInstalled(paths: paths) ? .installed : .notInstalled
        self.serviceStatuses = Dictionary(uniqueKeysWithValues: ServiceKind.allCases.map {
            ($0, ServiceStatus(state: .stopped, detail: ""))
        })
        self.analyticsOptIn = UserDefaults.standard.bool(forKey: "analyticsOptIn")
        self.logPreview = logStore.readTail(maxBytes: 12_000)
        self.logHandle = logStore.openForAppending()
        self.ensureRepoDirectories()
        self.refreshGitAvailability()
    }

    var appStatus: AppStatus {
        if serviceStatuses.values.contains(where: { $0.state == .failed }) {
            return .error
        }
        if serviceStatuses.values.allSatisfy({ $0.state == .running }) {
            return .running
        }
        if serviceStatuses.values.contains(where: { $0.state == .starting }) {
            return .starting
        }
        if serviceStatuses.values.contains(where: { $0.state == .stopping }) {
            return .stopping
        }
        return .stopped
    }

    var statusLabel: String {
        if installState == .notInstalled {
            return "Not installed"
        }
        return appStatus.label
    }

    var installStateLabel: String {
        installState.label
    }

    var installStateTone: StatusTone {
        installState.tone
    }

    var webhookStatusLabel: String {
        if config.webhookStatus.isEmpty {
            return "unknown"
        }
        return config.webhookStatus
    }

    var canStart: Bool {
        installState == .installed && serviceStatuses.values.contains(where: { $0.state != .running })
    }

    var canStop: Bool {
        serviceStatuses.values.contains(where: { $0.state == .running || $0.state == .starting })
    }

    var mcpServerURL: String {
        if !config.tunnelBaseURL.isEmpty {
            return config.tunnelBaseURL.trimmedSlashSuffix() + "/mcp"
        }
        return "http://localhost:\(config.mcpPort)/mcp"
    }

    var webhookURL: String {
        if !config.tunnelBaseURL.isEmpty {
            return config.tunnelBaseURL.trimmedSlashSuffix() + "/github/webhook"
        }
        return "http://localhost:\(config.webhookPort)/github/webhook"
    }

    var mcpServerURLBinding: Binding<String> {
        Binding(get: { self.mcpServerURL }, set: { _ in })
    }

    var webhookURLBinding: Binding<String> {
        Binding(get: { self.webhookURL }, set: { _ in })
    }

    var lovableInstructions: String {
        let repo = normalizedRepoFullName() ?? "<ORG/REPO>"
        let branch = config.defaultBranch.isEmpty ? "main" : config.defaultBranch
        let serverURL = mcpServerURL
        return """
        Prune MCP Server: \(serverURL)

        Before generating context:
        - call repo.ensure_fresh(repo: \"\(repo)\", branch: \"\(branch)\")

        After pushing commits:
        - call repo.sync(repo: \"\(repo)\", branch: \"\(branch)\", expected_sha: \"<sha>\")

        Then use context.pack for the task.
        """
    }

    var lovableInstructionsBinding: Binding<String> {
        Binding(get: { self.lovableInstructions }, set: { _ in })
    }

    func binding<T>(_ keyPath: WritableKeyPath<AppConfig, T>) -> Binding<T> {
        Binding(
            get: { self.config[keyPath: keyPath] },
            set: { newValue in
                self.config[keyPath: keyPath] = newValue
                self.saveConfig()
            }
        )
    }

    func refreshGitAvailability() {
        Task { @MainActor in
            self.gitAvailability = await self.checkGitAvailability()
        }
    }

    func installCommandLineTools() {
        statusMessage = "Requesting Xcode Command Line Tools installer..."
        lastErrorMessage = nil
        Task { @MainActor in
            do {
                _ = try await self.runCommand("/usr/bin/xcode-select", args: ["--install"])
                self.statusMessage = "Command Line Tools installer opened."
            } catch {
                self.statusMessage = "Command Line Tools: \(error.localizedDescription)"
            }
            self.refreshGitAvailability()
        }
    }

    func repoBinding() -> Binding<String> {
        Binding(
            get: { self.config.repoFullName },
            set: { newValue in
                self.config.repoFullName = newValue
                self.saveConfig()
                self.ensureRepoDirectories()
            }
        )
    }

    func detectRepoFromMirror() {
        statusMessage = "Detecting repository..."
        lastErrorMessage = nil

        Task { @MainActor in
            do {
                if let repo = normalizedRepoFullName() {
                    let mirror = paths.mirrorDirectory(repoFullName: repo)
                    if let candidate = try await repoCandidate(from: mirror) {
                        applyRepoCandidate(candidate)
                        return
                    }
                }

                let candidates = try await repoCandidatesFromMirrors()
                guard !candidates.isEmpty else {
                    statusMessage = "No repository detected in mirrors."
                    return
                }
                if candidates.count > 1 {
                    let names = candidates.map { $0.repoFullName }.sorted().joined(separator: ", ")
                    statusMessage = "Multiple repos found: \(names)."
                    return
                }
                applyRepoCandidate(candidates[0])
            } catch {
                lastErrorMessage = "Repository detection failed: \(error.localizedDescription)"
            }
        }
    }

    func binaryBinding(_ keyPath: WritableKeyPath<BinaryNames, String>) -> Binding<String> {
        Binding(
            get: { self.config.binaryNames[keyPath: keyPath] },
            set: { newValue in
                self.config.binaryNames[keyPath: keyPath] = newValue
                self.saveConfig()
            }
        )
    }

    func argumentsBinding(for service: ServiceKind) -> Binding<String> {
        Binding(
            get: {
                self.config.serviceArguments.args(for: service).joined(separator: "\n")
            },
            set: { newValue in
                let args = newValue
                    .split(whereSeparator: \.isNewline)
                    .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
                    .filter { !$0.isEmpty }
                self.config.serviceArguments.setArgs(args, for: service)
                self.saveConfig()
            }
        )
    }

    func install(reinstallOnly: Bool = false) {
        installState = .installing
        statusMessage = reinstallOnly ? "Reinstalling binaries..." : "Installing..."
        lastErrorMessage = nil

        Task {
            do {
                let result = try Installer.install(
                    paths: paths,
                    config: config,
                    reinstallOnly: reinstallOnly,
                    logStore: logStore
                )
                try configStore.save(config)
                installState = .installed
                ensureRepoDirectories()
                if result.warnings.isEmpty {
                    statusMessage = "Install completed."
                } else {
                    statusMessage = "Install completed with warnings."
                }
            } catch {
                installState = .failed
                lastErrorMessage = "Install failed: \(error.localizedDescription)"
            }
        }
    }

    func startServices() {
        guard installState == .installed else {
            lastErrorMessage = "Install Prune before starting services."
            return
        }
        statusMessage = "Starting services..."
        lastErrorMessage = nil
        if config.useLaunchAgents {
            startServicesWithLaunchAgents()
        } else {
            startService(.tunnel)
            startService(.sync)
            startService(.mcp)
            performHealthChecks()
        }
    }

    func stopServices() {
        statusMessage = "Stopping services..."
        lastErrorMessage = nil
        if config.useLaunchAgents {
            stopServicesWithLaunchAgents()
        } else {
            stopService(.mcp)
            stopService(.sync)
            stopService(.tunnel)
        }
    }

    func refreshStatus() {
        if config.useLaunchAgents {
            Task {
                await refreshLaunchAgentStatus()
            }
            return
        }

        for service in ServiceKind.allCases {
            if let process = processes[service] {
                if process.isRunning {
                    serviceStatuses[service]?.state = .running
                    serviceStatuses[service]?.detail = "PID \(process.processIdentifier)"
                } else {
                    serviceStatuses[service]?.state = .stopped
                    serviceStatuses[service]?.detail = ""
                    processes[service] = nil
                }
            } else {
                serviceStatuses[service]?.state = .stopped
            }
        }
        refreshLogPreview()
        refreshSyncStatus()
        ensureMcpRunningIfNeeded()
    }

    func installLaunchAgents() {
        statusMessage = "Installing LaunchAgents..."
        lastErrorMessage = nil

        Task {
            do {
                try FileManager.default.createDirectory(
                    at: paths.launchAgents,
                    withIntermediateDirectories: true,
                    attributes: nil
                )
                let domain = launchctlDomain()
                var warnings: [String] = []

                for service in ServiceKind.allCases {
                    do {
                        let plistURL = try writeLaunchAgentPlist(for: service)
                        _ = try? await runLaunchctl(["bootout", domain, plistURL.path])
                        _ = try await runLaunchctl(["bootstrap", domain, plistURL.path])
                    } catch {
                        warnings.append("\(service.displayName): \(error.localizedDescription)")
                    }
                }

                if warnings.isEmpty {
                    statusMessage = "LaunchAgents installed."
                } else {
                    statusMessage = "LaunchAgents installed with warnings."
                }
            } catch {
                lastErrorMessage = "LaunchAgent install failed: \(error.localizedDescription)"
            }
        }
    }

    func removeLaunchAgents() {
        statusMessage = "Removing LaunchAgents..."
        lastErrorMessage = nil

        Task {
            let domain = launchctlDomain()
            for service in ServiceKind.allCases {
                let label = launchAgentLabel(for: service)
                let plistURL = paths.launchAgentPlist(label: label)
                _ = try? await runLaunchctl(["bootout", domain, plistURL.path])
                if FileManager.default.fileExists(atPath: plistURL.path) {
                    try? FileManager.default.removeItem(at: plistURL)
                }
            }
            statusMessage = "LaunchAgents removed."
            await refreshLaunchAgentStatus()
        }
    }

    func openLaunchAgentsFolder() {
        NSWorkspace.shared.open(paths.launchAgents)
    }

    func copyLovableInstructions() {
        copyToClipboard(lovableInstructions)
    }

    private func startServicesWithLaunchAgents() {
        Task {
            do {
                try await ensureLaunchAgentsInstalled()
                let domain = launchctlDomain()
                _ = try await runLaunchctl(["kickstart", "-k", "\(domain)/\(launchAgentLabel(for: .tunnel))"])
                _ = try await runLaunchctl(["kickstart", "-k", "\(domain)/\(launchAgentLabel(for: .sync))"])
                _ = try await runLaunchctl(["kickstart", "-k", "\(domain)/\(launchAgentLabel(for: .mcp))"])
                await refreshLaunchAgentStatus()
                performHealthChecks()
            } catch {
                lastErrorMessage = "LaunchAgent start failed: \(error.localizedDescription)"
            }
        }
    }

    private func stopServicesWithLaunchAgents() {
        Task {
            let domain = launchctlDomain()
            _ = try? await runLaunchctl(["stop", "\(domain)/\(launchAgentLabel(for: .mcp))"])
            _ = try? await runLaunchctl(["stop", "\(domain)/\(launchAgentLabel(for: .sync))"])
            _ = try? await runLaunchctl(["stop", "\(domain)/\(launchAgentLabel(for: .tunnel))"])
            await refreshLaunchAgentStatus()
        }
    }

    private func refreshLaunchAgentStatus() async {
        let domain = launchctlDomain()
        for service in ServiceKind.allCases {
            let label = launchAgentLabel(for: service)
            do {
                let output = try await runLaunchctl(["print", "\(domain)/\(label)"])
                serviceStatuses[service] = parseLaunchctlStatus(output, service: service)
            } catch {
                serviceStatuses[service] = ServiceStatus(state: .stopped, detail: "launchd not loaded")
            }
        }
        refreshLogPreview()
        refreshSyncStatus()
    }

    private func performHealthChecks() {
        Task {
            let mcpResult = await checkEndpoint(urlString: mcpServerURL)
            if let mcpResult {
                statusMessage = "MCP \(mcpResult)"
            }
            let webhookResult = await checkEndpoint(urlString: webhookURL)
            if let webhookResult {
                let prefix = statusMessage ?? ""
                statusMessage = prefix.isEmpty ? "Webhook \(webhookResult)" : "\(prefix) | Webhook \(webhookResult)"
            }
        }
    }

    func serviceStatus(for service: ServiceKind) -> ServiceStatus {
        serviceStatuses[service] ?? ServiceStatus(state: .stopped, detail: "")
    }

    func openSettings(_ tab: SettingsTab) {
        selectedTab = tab
        NSApp.activate(ignoringOtherApps: true)
        _ = NSApp.sendAction(Selector(("showSettingsWindow:")), to: nil, from: nil)
    }

    func openLogs() {
        do {
            try logStore.ensureLogFile()
        } catch {
            lastErrorMessage = "Unable to create log file: \(error.localizedDescription)"
            return
        }
        NSWorkspace.shared.open(paths.logFile)
    }

    func refreshLogPreview() {
        logPreview = logStore.readTail(maxBytes: 12_000)
    }

    func refreshSyncStatus() {
        guard let data = try? Data(contentsOf: paths.syncStatusFile) else {
            return
        }
        guard let status = try? JSONDecoder().decode(SyncStatusFile.self, from: data) else {
            return
        }
        var didChange = false
        if let sha = status.lastIndexedSha, sha != config.lastIndexedSha {
            config.lastIndexedSha = sha
            didChange = true
        }
        if let error = status.lastError, !error.isEmpty {
            if config.webhookStatus != "error" {
                config.webhookStatus = "error"
                didChange = true
            }
        } else if status.lastEvent != nil {
            if config.webhookStatus != "ok" {
                config.webhookStatus = "ok"
                didChange = true
            }
        }
        if didChange {
            saveConfig()
        }
    }

    func copyToClipboard(_ value: String) {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(value, forType: .string)
        statusMessage = "Copied to clipboard."
    }

    func copyMcpURL() {
        copyToClipboard(mcpServerURL)
    }

    func copyWebhookURL() {
        copyToClipboard(webhookURL)
    }

    func testMcpConnection() {
        guard let url = URL(string: mcpServerURL) else {
            mcpTestStatus = "Invalid MCP URL."
            return
        }
        mcpTestStatus = "Testing..."
        let token = keychain.read(account: KeychainAccount.mcpToken)

        Task {
            var request = URLRequest(url: url)
            request.httpMethod = "GET"
            if let token, !token.isEmpty {
                request.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
            }
            do {
                let (_, response) = try await URLSession.shared.data(for: request)
                if let http = response as? HTTPURLResponse {
                    if (200..<500).contains(http.statusCode) {
                        mcpTestStatus = "MCP reachable (HTTP \(http.statusCode))."
                    } else {
                        mcpTestStatus = "MCP returned HTTP \(http.statusCode)."
                    }
                } else {
                    mcpTestStatus = "MCP reachable."
                }
            } catch {
                mcpTestStatus = "MCP test failed: \(error.localizedDescription)"
            }
        }
    }

    func saveGitHubToken() {
        saveSecret(githubTokenInput, account: KeychainAccount.githubToken, label: "GitHub token")
        githubTokenInput = ""
    }

    func saveWebhookSecret() {
        saveSecret(webhookSecretInput, account: KeychainAccount.webhookSecret, label: "Webhook secret")
        webhookSecretInput = ""
    }

    func saveMcpToken() {
        saveSecret(mcpTokenInput, account: KeychainAccount.mcpToken, label: "MCP token")
        mcpTokenInput = ""
    }

    func createGitHubWebhook() {
        guard let repo = normalizedRepoFullName() else {
            githubStatusMessage = "Set repository as ORG/REPO."
            return
        }
        guard let token = keychain.read(account: KeychainAccount.githubToken), !token.isEmpty else {
            githubStatusMessage = "Save a GitHub token first."
            return
        }
        guard let secret = keychain.read(account: KeychainAccount.webhookSecret), !secret.isEmpty else {
            githubStatusMessage = "Save a webhook secret first."
            return
        }

        githubStatusMessage = "Creating webhook..."
        Task {
            do {
                let hook = try await GitHubClient(token: token)
                    .createWebhook(repoFullName: repo, callbackURL: webhookURL, secret: secret)
                webhooks.append(hook)
                githubStatusMessage = "Webhook created (ID \(hook.id))."
            } catch {
                githubStatusMessage = "Create webhook failed: \(error.localizedDescription)"
            }
        }
    }

    func refreshGitHubWebhooks() {
        guard let repo = normalizedRepoFullName() else {
            githubStatusMessage = "Set repository as ORG/REPO."
            return
        }
        guard let token = keychain.read(account: KeychainAccount.githubToken), !token.isEmpty else {
            githubStatusMessage = "Save a GitHub token first."
            return
        }

        githubStatusMessage = "Fetching webhooks..."
        Task {
            do {
                let hooks = try await GitHubClient(token: token).listWebhooks(repoFullName: repo)
                webhooks = hooks
                githubStatusMessage = "Found \(hooks.count) webhooks."
            } catch {
                githubStatusMessage = "Fetch webhooks failed: \(error.localizedDescription)"
            }
        }
    }

    func deleteGitHubWebhook(id: Int) {
        guard let repo = normalizedRepoFullName() else {
            githubStatusMessage = "Set repository as ORG/REPO."
            return
        }
        guard let token = keychain.read(account: KeychainAccount.githubToken), !token.isEmpty else {
            githubStatusMessage = "Save a GitHub token first."
            return
        }

        githubStatusMessage = "Deleting webhook..."
        Task {
            do {
                try await GitHubClient(token: token).deleteWebhook(repoFullName: repo, id: id)
                webhooks.removeAll { $0.id == id }
                githubStatusMessage = "Webhook deleted."
            } catch {
                githubStatusMessage = "Delete webhook failed: \(error.localizedDescription)"
            }
        }
    }

    func exportDiagnostics() {
        do {
            let folderURL = try DiagnosticsExporter.export(
                paths: paths,
                config: config,
                serviceStatuses: serviceStatuses,
                logStore: logStore
            )
            lastDiagnosticsPath = folderURL
            NSWorkspace.shared.activateFileViewerSelecting([folderURL])
            statusMessage = "Diagnostics exported."
        } catch {
            lastErrorMessage = "Export failed: \(error.localizedDescription)"
        }
    }

    static func preview() -> AppModel {
        let model = AppModel()
        model.installState = .installed
        model.serviceStatuses[.tunnel] = ServiceStatus(state: .running, detail: "PID 1201")
        model.serviceStatuses[.sync] = ServiceStatus(state: .running, detail: "PID 1202")
        model.serviceStatuses[.mcp] = ServiceStatus(state: .running, detail: "PID 1203")
        model.statusMessage = "Ready."
        return model
    }

    private func saveConfig() {
        do {
            try configStore.save(config)
        } catch {
            lastErrorMessage = "Failed to save config: \(error.localizedDescription)"
        }
    }

    private func processEnvironment(for service: ServiceKind) -> [String: String] {
        var env = ProcessInfo.processInfo.environment
        env["PATH"] = Self.defaultPath(env["PATH"])
        env["PRUNE_HOME"] = paths.appSupport.path
        env["PRUNE_LOG_DIR"] = paths.logs.path
        if service == .sync {
            if let secret = keychain.read(account: KeychainAccount.webhookSecret), !secret.isEmpty {
                env["PRUNE_WEBHOOK_SECRET"] = secret
            }
            if let token = keychain.read(account: KeychainAccount.githubToken), !token.isEmpty {
                env["GITHUB_TOKEN"] = token
            }
        }
        if service == .mcp {
            if let token = keychain.read(account: KeychainAccount.mcpToken), !token.isEmpty {
                env["PRUNE_MCP_TOKEN"] = token
            }
        }
        return env
    }

    private func launchAgentEnvironment(for service: ServiceKind) -> [String: String] {
        var env: [String: String] = [
            "PATH": Self.defaultPath(nil),
            "PRUNE_HOME": paths.appSupport.path,
            "PRUNE_LOG_DIR": paths.logs.path
        ]
        if let home = ProcessInfo.processInfo.environment["HOME"], !home.isEmpty {
            env["HOME"] = home
        }
        if service == .sync {
            if let secret = keychain.read(account: KeychainAccount.webhookSecret), !secret.isEmpty {
                env["PRUNE_WEBHOOK_SECRET"] = secret
            }
            if let token = keychain.read(account: KeychainAccount.githubToken), !token.isEmpty {
                env["GITHUB_TOKEN"] = token
            }
        }
        if service == .mcp {
            if let token = keychain.read(account: KeychainAccount.mcpToken), !token.isEmpty {
                env["PRUNE_MCP_TOKEN"] = token
            }
        }
        return env
    }

    private static func defaultPath(_ existing: String?) -> String {
        let fallback = "/usr/bin:/bin:/usr/sbin:/sbin:/usr/local/bin:/opt/homebrew/bin"
        if let existing, !existing.isEmpty {
            return "\(existing):\(fallback)"
        }
        return fallback
    }

    private func ensureRepoDirectories() {
        guard let repo = normalizedRepoFullName() else { return }
        let fileManager = FileManager.default
        do {
            try fileManager.createDirectory(
                at: paths.mirrorDirectory(repoFullName: repo),
                withIntermediateDirectories: true,
                attributes: nil
            )
            try fileManager.createDirectory(
                at: paths.ceIndexDirectory(repoFullName: repo),
                withIntermediateDirectories: true,
                attributes: nil
            )
            try fileManager.createDirectory(
                at: paths.ceHnswDirectory(repoFullName: repo),
                withIntermediateDirectories: true,
                attributes: nil
            )
        } catch {
            logStore.append("Failed to create repo directories: \(error.localizedDescription)")
        }
    }

    private func launchctlDomain() -> String {
        "gui/\(getuid())"
    }

    private func launchAgentLabel(for service: ServiceKind) -> String {
        let base = Bundle.main.bundleIdentifier ?? "com.prune.app"
        return "\(base).\(service.rawValue)"
    }

    private func writeLaunchAgentPlist(for service: ServiceKind) throws -> URL {
        let label = launchAgentLabel(for: service)
        let plistURL = paths.launchAgentPlist(label: label)
        let binaryName = binaryName(for: service)
        let binaryURL = binaryURL(for: service)
        guard FileManager.default.isExecutableFile(atPath: binaryURL.path) else {
            throw NSError(domain: "PruneApp", code: 2, userInfo: [
                NSLocalizedDescriptionKey: "Missing binary: \(binaryName)"
            ])
        }

        let args = [binaryURL.path] + arguments(for: service)
        var plist: [String: Any] = [
            "Label": label,
            "ProgramArguments": args,
            "RunAtLoad": true,
            "KeepAlive": true,
            "WorkingDirectory": workingDirectory(for: service).path,
            "StandardOutPath": paths.logFile.path,
            "StandardErrorPath": paths.logFile.path
        ]
        let env = launchAgentEnvironment(for: service)
        if !env.isEmpty {
            plist["EnvironmentVariables"] = env
        }

        let data = try PropertyListSerialization.data(fromPropertyList: plist, format: .xml, options: 0)
        try data.write(to: plistURL, options: [.atomic])
        return plistURL
    }

    private func ensureLaunchAgentsInstalled() async throws {
        try FileManager.default.createDirectory(
            at: paths.launchAgents,
            withIntermediateDirectories: true,
            attributes: nil
        )
        let domain = launchctlDomain()

        for service in ServiceKind.allCases {
            let plistURL = try writeLaunchAgentPlist(for: service)
            _ = try? await runLaunchctl(["bootout", domain, plistURL.path])
            _ = try await runLaunchctl(["bootstrap", domain, plistURL.path])
        }
    }

    private func runLaunchctl(_ args: [String]) async throws -> String {
        try await runCommand("/bin/launchctl", args: args)
    }

    private func runCommand(_ command: String, args: [String]) async throws -> String {
        try await Task.detached {
            let process = Process()
            process.executableURL = URL(fileURLWithPath: command)
            process.arguments = args
            let pipe = Pipe()
            process.standardOutput = pipe
            process.standardError = pipe
            try process.run()
            process.waitUntilExit()
            let data = pipe.fileHandleForReading.readDataToEndOfFile()
            let output = String(data: data, encoding: .utf8) ?? ""
            if process.terminationStatus != 0 {
                throw NSError(domain: "PruneApp", code: Int(process.terminationStatus), userInfo: [
                    NSLocalizedDescriptionKey: output.isEmpty ? "Command failed" : output
                ])
            }
            return output
        }.value
    }

    private func checkGitAvailability() async -> ToolAvailability {
        do {
            let output = try await runCommand("/usr/bin/xcrun", args: ["--find", "git"])
            let path = output
                .split(whereSeparator: { $0 == "\n" || $0 == "\r" })
                .first
                .map(String.init) ?? ""
            guard !path.isEmpty, FileManager.default.isExecutableFile(atPath: path) else {
                return .missing("Git not found. Install Xcode Command Line Tools.")
            }
            return .available
        } catch {
            return .missing("Git not found. Install Xcode Command Line Tools.")
        }
    }

    private func parseLaunchctlStatus(_ output: String, service: ServiceKind) -> ServiceStatus {
        let lines = output.split(separator: "\n")
        var pid: String?
        var state: String?
        for line in lines {
            let trimmed = line.trimmingCharacters(in: .whitespaces)
            if trimmed.hasPrefix("pid =") {
                pid = trimmed.replacingOccurrences(of: "pid =", with: "").trimmingCharacters(in: .whitespaces)
            } else if trimmed.hasPrefix("state =") {
                state = trimmed.replacingOccurrences(of: "state =", with: "").trimmingCharacters(in: .whitespaces)
            }
        }

        if let pid, !pid.isEmpty, pid != "0" {
            return ServiceStatus(state: .running, detail: "PID \(pid) (launchd)")
        }
        if state == "running" {
            return ServiceStatus(state: .running, detail: "launchd running")
        }
        return ServiceStatus(state: .stopped, detail: "launchd idle")
    }

    private func checkEndpoint(urlString: String) async -> String? {
        guard let url = URL(string: urlString), let scheme = url.scheme else {
            return "invalid URL"
        }
        guard scheme == "http" || scheme == "https" else {
            return "non-HTTP URL"
        }

        do {
            var request = URLRequest(url: url)
            request.httpMethod = "GET"
            let (_, response) = try await URLSession.shared.data(for: request)
            if let http = response as? HTTPURLResponse {
                return "HTTP \(http.statusCode)"
            }
            return "reachable"
        } catch {
            return "unreachable"
        }
    }

    private func arguments(for service: ServiceKind) -> [String] {
        let extraArgs = config.serviceArguments.args(for: service)
        return defaultArguments(for: service) + extraArgs
    }

    private func defaultArguments(for service: ServiceKind) -> [String] {
        switch service {
        case .mcp:
            guard let repo = normalizedRepoFullName() else { return [] }
            let binaryName = config.binaryNames.mcp.lowercased()
            let dbPath = paths.ceIndexDatabase(repoFullName: repo).path
            let hnswPath = paths.ceHnswDirectory(repoFullName: repo).path
            if binaryName.contains("prune-mcp") {
                let ceMcpPath = paths.bin.appendingPathComponent("ce-mcp").path
                let syncURL = "http://127.0.0.1:\(config.webhookPort)"
                return [
                    "--bind", "127.0.0.1:\(config.mcpPort)",
                    "--db", dbPath,
                    "--hnsw-dir", hnswPath,
                    "--ce-mcp-path", ceMcpPath,
                    "--sync-url", syncURL
                ]
            }
            if binaryName.contains("ce-mcp") {
                return ["--db", dbPath, "--hnsw-dir", hnswPath]
            }
            return []
        case .sync:
            guard let repo = normalizedRepoFullName() else { return [] }
            let binaryName = config.binaryNames.sync.lowercased()
            guard binaryName.contains("prune-sync") else { return [] }
            let mirrorPath = paths.mirrorDirectory(repoFullName: repo).path
            let dbPath = paths.ceIndexDatabase(repoFullName: repo).path
            let hnswPath = paths.ceHnswDirectory(repoFullName: repo).path
            let statusPath = paths.syncStatusFile.path
            let cePath = paths.bin.appendingPathComponent("ce").path
            return [
                "--bind", "127.0.0.1:\(config.webhookPort)",
                "--repo", repo,
                "--branch", config.defaultBranch,
                "--mirror-dir", mirrorPath,
                "--db", dbPath,
                "--hnsw-dir", hnswPath,
                "--status-file", statusPath,
                "--ce-path", cePath,
                "--prune"
            ]
        case .tunnel:
            let binaryName = config.binaryNames.tunnel.lowercased()
            guard binaryName.contains("cloudflared") else { return [] }
            let useMcpPort = config.binaryNames.mcp.lowercased().contains("prune-mcp")
            let port = useMcpPort ? config.mcpPort : config.webhookPort
            let localURL = "http://127.0.0.1:\(port)"
            return ["tunnel", "--no-autoupdate", "--url", localURL]
        }
    }

    private func workingDirectory(for service: ServiceKind) -> URL {
        if let repo = normalizedRepoFullName() {
            return paths.mirrorDirectory(repoFullName: repo)
        }
        return paths.appSupport
    }

    private func binaryName(for service: ServiceKind) -> String {
        switch service {
        case .tunnel:
            return config.binaryNames.tunnel
        case .sync:
            return config.binaryNames.sync
        case .mcp:
            return config.binaryNames.mcp
        }
    }

    private func binaryURL(for service: ServiceKind) -> URL {
        let name = binaryName(for: service)
        if name.hasPrefix("/") {
            return URL(fileURLWithPath: name)
        }
        return paths.bin.appendingPathComponent(name)
    }

    private func startService(_ service: ServiceKind) {
        if let existing = processes[service], existing.isRunning {
            serviceStatuses[service] = ServiceStatus(state: .running, detail: "PID \(existing.processIdentifier)")
            return
        }
        processes[service] = nil

        if service == .tunnel && isCloudflaredTunnel() {
            startCloudflaredTunnel()
            return
        }

        let binaryName = binaryName(for: service)
        let binaryURL = binaryURL(for: service)
        guard FileManager.default.isExecutableFile(atPath: binaryURL.path) else {
            serviceStatuses[service] = ServiceStatus(state: .failed, detail: "Missing binary: \(binaryName)")
            return
        }

        let process = Process()
        process.executableURL = binaryURL
        process.arguments = arguments(for: service)
        process.currentDirectoryURL = workingDirectory(for: service)
        process.environment = processEnvironment(for: service)
        process.standardOutput = logHandle
        process.standardError = logHandle
        process.terminationHandler = { [weak self] _ in
            Task { @MainActor in
                self?.serviceStatuses[service] = ServiceStatus(state: .stopped, detail: "")
                self?.processes[service] = nil
            }
        }

        do {
            try process.run()
            processes[service] = process
            serviceStatuses[service] = ServiceStatus(state: .running, detail: "PID \(process.processIdentifier)")
            logStore.append("\(service.displayName) started.")
        } catch {
            serviceStatuses[service] = ServiceStatus(state: .failed, detail: error.localizedDescription)
        }
    }

    private func startCloudflaredTunnel() {
        let service = ServiceKind.tunnel
        let binaryName = binaryName(for: service)
        let binaryURL = binaryURL(for: service)
        guard FileManager.default.isExecutableFile(atPath: binaryURL.path) else {
            serviceStatuses[service] = ServiceStatus(state: .failed, detail: "Missing binary: \(binaryName)")
            return
        }

        let stdout = Pipe()
        let stderr = Pipe()
        let process = Process()
        process.executableURL = binaryURL
        process.arguments = arguments(for: service)
        process.currentDirectoryURL = workingDirectory(for: service)
        process.environment = processEnvironment(for: service)
        process.standardOutput = stdout
        process.standardError = stderr
        process.terminationHandler = { [weak self] _ in
            Task { @MainActor in
                self?.serviceStatuses[service] = ServiceStatus(state: .stopped, detail: "")
                self?.processes[service] = nil
            }
        }

        do {
            try process.run()
            processes[service] = process
            serviceStatuses[service] = ServiceStatus(state: .running, detail: "PID \(process.processIdentifier)")
            logStore.append("\(service.displayName) started.")
            monitorTunnelPipe(stdout, label: "stdout")
            monitorTunnelPipe(stderr, label: "stderr")
        } catch {
            serviceStatuses[service] = ServiceStatus(state: .failed, detail: error.localizedDescription)
        }
    }

    private func monitorTunnelPipe(_ pipe: Pipe, label: String) {
        let handle = pipe.fileHandleForReading
        Task { [weak self] in
            do {
                for try await line in handle.bytes.lines {
                    self?.logStore.append("[tunnel \(label)] \(line)")
                    if let url = Self.extractTunnelURL(from: line) {
                        if self?.config.tunnelBaseURL != url {
                            self?.config.tunnelBaseURL = url
                            self?.saveConfig()
                            self?.statusMessage = "Tunnel URL set."
                        }
                    }
                }
            } catch {
                self?.logStore.append("[tunnel \(label)] stream ended")
            }
        }
    }

    private static func extractTunnelURL(from line: String) -> String? {
        guard let range = line.range(of: "https://") else {
            return nil
        }
        let candidate = String(line[range.lowerBound...])
        if let end = candidate.firstIndex(of: " ") {
            return String(candidate[..<end])
        }
        return candidate
    }

    private func isCloudflaredTunnel() -> Bool {
        let name = URL(fileURLWithPath: binaryName(for: .tunnel))
            .lastPathComponent
            .lowercased()
        return name.contains("cloudflared")
    }

    private func stopService(_ service: ServiceKind) {
        guard let process = processes[service] else {
            serviceStatuses[service] = ServiceStatus(state: .stopped, detail: "")
            return
        }
        serviceStatuses[service] = ServiceStatus(state: .stopping, detail: "Stopping...")
        process.terminate()
        Task {
            try? await Task.sleep(nanoseconds: 500_000_000)
            if process.isRunning {
                process.interrupt()
            }
        }
    }

    private func saveSecret(_ value: String, account: String, label: String) {
        guard !value.isEmpty else {
            statusMessage = "\(label) is empty."
            return
        }
        do {
            try keychain.save(value, account: account)
            statusMessage = "\(label) saved."
        } catch {
            lastErrorMessage = "Failed to save \(label): \(error.localizedDescription)"
        }
    }

    private func ensureMcpRunningIfNeeded() {
        guard installState == .installed else { return }
        guard !config.useLaunchAgents else { return }
        guard serviceStatuses[.mcp]?.state != .running else { return }
        let syncRunning = serviceStatuses[.sync]?.state == .running
        let tunnelRunning = serviceStatuses[.tunnel]?.state == .running
        guard syncRunning || tunnelRunning else { return }
        startService(.mcp)
    }

    private struct RepoCandidate {
        let repoFullName: String
        let defaultBranch: String?
    }

    private func applyRepoCandidate(_ candidate: RepoCandidate) {
        config.repoFullName = candidate.repoFullName
        if let branch = candidate.defaultBranch, !branch.isEmpty {
            config.defaultBranch = branch
        }
        saveConfig()
        ensureRepoDirectories()
        statusMessage = "Repository set to \(candidate.repoFullName)."
    }

    private func repoCandidate(from directory: URL) async throws -> RepoCandidate? {
        guard FileManager.default.fileExists(atPath: directory.path) else {
            return nil
        }
        guard let repo = try await repoFromGit(directory: directory) else {
            return nil
        }
        let branch = await defaultBranch(from: directory)
        return RepoCandidate(repoFullName: repo, defaultBranch: branch)
    }

    private func repoCandidatesFromMirrors() async throws -> [RepoCandidate] {
        guard FileManager.default.fileExists(atPath: paths.mirrors.path) else {
            return []
        }
        let directories = try FileManager.default.contentsOfDirectory(
            at: paths.mirrors,
            includingPropertiesForKeys: [.isDirectoryKey],
            options: [.skipsHiddenFiles]
        )
        var candidates: [RepoCandidate] = []
        for directory in directories {
            let values = try directory.resourceValues(forKeys: [.isDirectoryKey])
            guard values.isDirectory == true else { continue }
            if let candidate = try await repoCandidate(from: directory) {
                candidates.append(candidate)
            }
        }
        return candidates
    }

    private func repoFromGit(directory: URL) async throws -> String? {
        if let repo = try? await runCommand(
            "/usr/bin/git",
            args: ["-C", directory.path, "remote", "get-url", "origin"]
        ) {
            if let parsed = parseGitRemoteURL(repo) {
                return parsed
            }
        }

        let output = try await runCommand(
            "/usr/bin/git",
            args: ["-C", directory.path, "remote", "-v"]
        )
        return parseGitRemoteOutput(output)
    }

    private func parseGitRemoteOutput(_ output: String) -> String? {
        var fallback: String?
        for line in output.split(separator: "\n") {
            let parts = line.split(whereSeparator: \.isWhitespace)
            guard parts.count >= 2 else { continue }
            let name = parts[0]
            let url = String(parts[1])
            let kind = parts.count >= 3 ? String(parts[2]) : ""
            if name == "origin", kind.contains("(fetch)") {
                return parseGitRemoteURL(url)
            }
            if fallback == nil {
                fallback = parseGitRemoteURL(url)
            }
        }
        return fallback
    }

    private func parseGitRemoteURL(_ raw: String) -> String? {
        var value = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !value.isEmpty else { return nil }
        if value.hasSuffix(".git") {
            value = String(value.dropLast(4))
        }
        value = value.trimmedSlashSuffix()

        let path: String
        if let schemeRange = value.range(of: "://") {
            let afterScheme = value[schemeRange.upperBound...]
            guard let slash = afterScheme.firstIndex(of: "/") else { return nil }
            path = String(afterScheme[slash...])
        } else if let colon = value.firstIndex(of: ":") {
            path = String(value[value.index(after: colon)...])
        } else {
            path = value
        }

        let components = path.split(separator: "/").filter { !$0.isEmpty }
        guard components.count >= 2 else { return nil }
        let org = components[components.count - 2]
        let repo = components[components.count - 1]
        return "\(org)/\(repo)"
    }

    private func defaultBranch(from directory: URL) async -> String? {
        guard let output = try? await runCommand(
            "/usr/bin/git",
            args: ["-C", directory.path, "symbolic-ref", "refs/remotes/origin/HEAD"]
        ) else {
            return nil
        }
        let trimmed = output.trimmingCharacters(in: .whitespacesAndNewlines)
        let parts = trimmed.split(separator: "/")
        return parts.last.map(String.init)
    }

    /// Best-effort: resolves the current mirror directory for the configured repo.
    ///
    /// The mirror directory is where Prune keeps a local clone used by MCP and indexing.
    func currentMirrorDirectory() -> URL? {
        guard let repo = normalizedRepoFullName() else { return nil }
        return paths.mirrorDirectory(repoFullName: repo)
    }

    /// Runs the bundled `ce` CLI (Context Engine) and returns combined stdout/stderr.
    ///
    /// This is used by the Inception flow to `ce bootstrap` and (optionally) `ce index`.
    func runCe(_ args: [String]) async throws -> String {
        let cePath = paths.bin.appendingPathComponent("ce").path
        return try await runCommand(cePath, args: args)
    }

    func normalizedRepoFullName() -> String? {
        let trimmed = config.repoFullName.trimmingCharacters(in: .whitespacesAndNewlines)
        let parts = trimmed.split(separator: "/")
        guard parts.count == 2 else { return nil }
        return "\(parts[0])/\(parts[1])"
    }
}

private extension String {
    func trimmedSlashSuffix() -> String {
        var value = self
        while value.hasSuffix("/") {
            value.removeLast()
        }
        return value
    }
}
