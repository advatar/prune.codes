//
//  PruneAppTests.swift
//  PruneAppTests
//
//  Created by Johan Sellström on 2026-01-17.
//

import Testing
@testable import PruneApp

struct PruneAppTests {

    @MainActor
    @Test func appStatusToneMapsOperationalState() {
        #expect(AppStatus.running.tone == .good)
        #expect(AppStatus.starting.tone == .warning)
        #expect(AppStatus.stopping.tone == .warning)
        #expect(AppStatus.error.tone == .bad)
        #expect(AppStatus.stopped.tone == .neutral)
    }

    @MainActor
    @Test func serviceMenuIconsAreStable() {
        #expect(ServiceKind.tunnel.menuSystemImage == "network")
        #expect(ServiceKind.sync.menuSystemImage == "arrow.triangle.2.circlepath")
        #expect(ServiceKind.mcp.menuSystemImage == "point.3.connected.trianglepath.dotted")
    }

    @MainActor
    @Test func webhookStatusToneClassifiesStatusText() {
        let model = AppModel.preview()

        model.config.webhookStatus = "ok"
        #expect(model.webhookStatusTone == .good)

        model.config.webhookStatus = "failed"
        #expect(model.webhookStatusTone == .bad)

        model.config.webhookStatus = "unknown"
        #expect(model.webhookStatusTone == .neutral)

        model.config.webhookStatus = "pending"
        #expect(model.webhookStatusTone == .warning)
    }
}
