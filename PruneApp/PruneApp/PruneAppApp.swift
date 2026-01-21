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
}

final class AppDelegate: NSObject, NSApplicationDelegate {
    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApplication.shared.setActivationPolicy(.accessory)
    }
}
