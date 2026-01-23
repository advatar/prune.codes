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
    @StateObject private var a2uiAgent = A2UIAgent()

    var body: some Scene {
        MenuBarExtra {
            MenuBarView()
                .environmentObject(appModel)
                .environmentObject(a2uiAgent)
        } label: {
            MenuBarLabel(status: appModel.appStatus)
        }

        Settings {
            SettingsView()
                .environmentObject(appModel)
                .environmentObject(a2uiAgent)
        }
    }
}

final class AppDelegate: NSObject, NSApplicationDelegate {
    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApplication.shared.setActivationPolicy(.accessory)
    }
}
