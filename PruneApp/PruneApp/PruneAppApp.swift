//
//  PruneAppApp.swift
//  PruneApp
//
//  Created by Johan Sellström on 2026-01-17.
//

import AppKit
import SwiftUI

@main
struct PruneAppApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var appDelegate
    @StateObject private var appModel = AppModel()

    var body: some Scene {
        WindowGroup("Prune", id: "dashboard") {
            SettingsView()
                .environmentObject(appModel)
        }
        .defaultSize(width: 940, height: 680)
        .commands {
            pruneCommands
        }

        MenuBarExtra {
            MenuBarView()
                .environmentObject(appModel)
        } label: {
            MenuBarLabel(status: appModel.appStatus)
        }

        Settings {
            SettingsView()
                .environmentObject(appModel)
        }
    }

    @CommandsBuilder
    private var pruneCommands: some Commands {
        CommandMenu("Prune") {
            Button {
                appModel.openSettings(.setup)
            } label: {
                Label("Open Dashboard", systemImage: "rectangle.grid.2x2")
            }
            .keyboardShortcut("0", modifiers: [.command])

            Divider()

            Button {
                appModel.startServices()
            } label: {
                Label("Start Services", systemImage: "play.fill")
            }
            .keyboardShortcut("r", modifiers: [.command, .shift])
            .disabled(!appModel.canStart)

            Button {
                appModel.stopServices()
            } label: {
                Label("Stop Services", systemImage: "stop.fill")
            }
            .keyboardShortcut(".", modifiers: [.command])
            .disabled(!appModel.canStop)

            Button {
                appModel.refreshStatus()
            } label: {
                Label("Refresh Status", systemImage: "arrow.clockwise")
            }
            .keyboardShortcut("r", modifiers: [.command])

            Divider()

            Button {
                appModel.copyMcpURL()
            } label: {
                Label("Copy MCP URL", systemImage: "doc.on.doc")
            }
            .keyboardShortcut("m", modifiers: [.command, .shift])

            Button {
                appModel.exportDiagnostics()
            } label: {
                Label("Export Diagnostics", systemImage: "square.and.arrow.up")
            }
            .keyboardShortcut("e", modifiers: [.command, .shift])
        }
    }
}

final class AppDelegate: NSObject, NSApplicationDelegate {
    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApp.setActivationPolicy(.regular)
        NSApp.activate(ignoringOtherApps: true)
    }

    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool {
        if !flag {
            sender.activate(ignoringOtherApps: true)
        }
        return true
    }
}
