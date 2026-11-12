import Foundation
import Observation
import SwiftUI
import Testing

@testable import Kura

/// A page of `count` items whose actions record what ran.
@MainActor @Observable
private final class FakePage: LauncherPage {
    let title: String
    let placeholder = "Search"
    var query = ""
    var layout: LauncherLayout
    var count: Int
    let emptyTitle = "Nothing"
    var resets = 0
    var ran: [String] = []
    var next: FakePage?

    init(_ title: String = "Root", count: Int = 12, layout: LauncherLayout = .grid(columns: 5)) {
        self.title = title
        self.count = count
        self.layout = layout
    }

    var items: [LauncherItem] {
        (0..<count).map { LauncherItem(id: "\($0)", title: "Item \($0)") }
    }

    func actions(for item: LauncherItem) -> [LauncherAction] {
        [
            LauncherAction(title: "Primary", systemImage: "a") { self.ran.append("primary \(item.id)"); return .close() },
            LauncherAction(title: "Secondary", systemImage: "b") { self.ran.append("secondary \(item.id)"); return .stay(message: "Done") },
            LauncherAction(title: "Open", systemImage: "c", shortcut: .command("o")) {
                self.ran.append("open \(item.id)")
                return self.next.map { .push($0) } ?? .stay()
            },
            LauncherAction(title: "Fail", systemImage: "d", shortcut: .command("f", shift: true)) {
                throw KuraError(code: "x", message: "nope")
            },
        ]
    }

    func reset() {
        resets += 1
        query = ""
    }
}

@MainActor
private func settle() async {
    for _ in 0..<5 { await Task.yield() }
}

struct GridMoveTests {
    @Test func movesAroundAGridAndStopsAtEdges() {
        // 12 items, 5 columns: rows 0-4, 5-9, 10-11.
        func go(_ move: GridMove, _ from: Int) -> Int { GridMove.apply(move, from: from, count: 12, columns: 5) }
        #expect(go(.right, 0) == 1)
        #expect(go(.left, 0) == 0)
        #expect(go(.right, 11) == 11)
        #expect(go(.down, 2) == 7)
        #expect(go(.up, 7) == 2)
        #expect(go(.up, 2) == 2)
        #expect(go(.down, 6) == 11)  // the short last row: its last item
        #expect(go(.down, 11) == 11)
        #expect(go(.down, 9) == 11)
        #expect(GridMove.apply(.down, from: 0, count: 0, columns: 5) == 0)
        #expect(GridMove.apply(.down, from: 0, count: 3, columns: 1) == 1)  // a list is one column
    }
}

struct LauncherKeyMapTests {
    private func command(_ key: KeyEquivalent, _ modifiers: EventModifiers = [], _ layout: LauncherLayout = .grid(columns: 5))
        -> LauncherCommand
    {
        LauncherCommand.from(key: key, modifiers: modifiers, layout: layout)
    }

    @Test func theKeysMeanTheSameOnEveryPage() {
        #expect(command(.upArrow) == .move(.up))
        #expect(command(.downArrow) == .move(.down))
        #expect(command(.leftArrow) == .move(.left))
        #expect(command(.rightArrow) == .move(.right))
        #expect(command(.return) == .primary)
        #expect(command(.return, .command) == .secondary)
        #expect(command(.escape) == .escape)
        #expect(command("k", .command) == .toggleActions)
        #expect(command("o", .command) == .shortcut(.command("o")))
        #expect(command("C", [.command, .shift]) == .shortcut(.command("c", shift: true)))
    }

    @Test func typingAndListCursorKeysStayTheSearchFields() {
        #expect(command("a") == .none)
        #expect(command("a", .shift) == .none)
        #expect(command(.leftArrow, [], .list) == .none)
        #expect(command(.rightArrow, [], .list) == .none)
        #expect(command(.downArrow, [], .list) == .move(.down))
    }

    @Test func shortcutLabels() {
        #expect(LauncherShortcut.command("o").label == "⌘O")
        #expect(LauncherShortcut.command("c", shift: true).label == "⇧⌘C")
    }
}

@MainActor
struct LauncherTests {
    @Test func arrowsMoveTheHighlightAndReturnRunsThePrimary() async {
        let page = FakePage()
        let launcher = Launcher(root: page)
        var closed = 0
        launcher.close = { closed += 1 }
        launcher.opened()
        launcher.handle(.move(.down))
        launcher.handle(.move(.right))
        #expect(launcher.current?.id == "6")
        launcher.handle(.primary)
        await settle()
        #expect(page.ran == ["primary 6"])
        #expect(closed == 1)
    }

    @Test func commandReturnRunsTheSecondaryAndShowsItsMessage() async {
        let page = FakePage()
        let launcher = Launcher(root: page)
        launcher.opened()
        launcher.handle(.secondary)
        await settle()
        #expect(page.ran == ["secondary 0"])
        #expect(launcher.message == "Done")
    }

    @Test func shortcutsRunTheirActionAndUnknownOnesAreNotUsed() async {
        let page = FakePage()
        let launcher = Launcher(root: page)
        launcher.opened()
        #expect(launcher.handle(.shortcut(.command("o"))))
        #expect(!launcher.handle(.shortcut(.command("z"))))
        await settle()
        #expect(page.ran == ["open 0"])
    }

    @Test func aFailedActionSaysSoAndStays() async {
        let launcher = Launcher(root: FakePage())
        var closed = 0
        launcher.close = { closed += 1 }
        launcher.opened()
        launcher.handle(.shortcut(.command("f", shift: true)))
        await settle()
        #expect(launcher.message == "Fail failed: nope")
        #expect(closed == 0)
    }

    @Test func theActionPanelTakesTheArrowsAndReturn() async {
        let page = FakePage()
        let launcher = Launcher(root: page)
        launcher.opened()
        launcher.handle(.toggleActions)
        #expect(launcher.actionsOpen)
        launcher.handle(.move(.down))
        launcher.handle(.move(.right))  // ignored in the panel
        #expect(launcher.actionHighlighted == 1)
        #expect(launcher.current?.id == "0")
        launcher.handle(.primary)
        await settle()
        #expect(page.ran == ["secondary 0"])
        #expect(!launcher.actionsOpen)
    }

    @Test func escapeStepsBackOneThingAtATime() async {
        let root = FakePage()
        root.next = FakePage("Sub")
        let launcher = Launcher(root: root)
        var closed = 0
        launcher.close = { closed += 1 }
        launcher.opened()
        launcher.handle(.shortcut(.command("o")))
        await settle()
        #expect(launcher.page.title == "Sub")
        launcher.query = "cat"
        launcher.handle(.toggleActions)

        launcher.handle(.escape)
        #expect(!launcher.actionsOpen && launcher.query == "cat")
        launcher.handle(.escape)
        #expect(launcher.query == "" && launcher.page.title == "Sub")
        launcher.handle(.escape)
        #expect(launcher.page.title == "Root" && closed == 0)
        launcher.handle(.escape)
        #expect(closed == 1)
    }

    @Test func typingStartsAgainAtTheTop() {
        let launcher = Launcher(root: FakePage())
        launcher.opened()
        launcher.handle(.move(.down))
        launcher.query = "x"
        #expect(launcher.highlighted == 0)
    }

    @Test func reopeningSoonKeepsYourPlaceLaterStartsFresh() {
        let page = FakePage()
        let launcher = Launcher(root: page)
        let start = Date(timeIntervalSince1970: 1_000_000)
        launcher.opened(at: start)
        #expect(page.resets == 1)
        launcher.query = "cat"
        launcher.handle(.move(.right))
        launcher.closed(at: start)

        launcher.opened(at: start.addingTimeInterval(10))
        #expect(launcher.query == "cat" && launcher.highlighted == 1 && page.resets == 1)
        launcher.closed(at: start.addingTimeInterval(10))

        launcher.opened(at: start.addingTimeInterval(10 + Launcher.memory + 1))
        #expect(launcher.query == "" && launcher.highlighted == 0 && page.resets == 2)
    }

    @Test func aShrinkingPageNeverHighlightsPastTheEnd() {
        let page = FakePage()
        let launcher = Launcher(root: page)
        launcher.opened()
        launcher.highlight(11)
        page.count = 3
        #expect(launcher.current?.id == "2")
        page.count = 0
        #expect(launcher.current == nil && launcher.actions.isEmpty)
        #expect(!launcher.handle(.none))
    }
}

@MainActor
struct UsageTests {
    private func image(_ hash: String) -> KuraImage {
        KuraImage(
            hash: hash, name: hash, mimeType: "image/png", createdAt: "", tags: [], fileUrl: "/\(hash)",
            thumbnailUrl: "")
    }

    @Test func usedMemesComeFirstMostFrecentFirstThenNewest() {
        let now = Date(timeIntervalSince1970: 10_000_000)
        let week: TimeInterval = 7 * 24 * 3600
        let entries: [String: Usage.Entry] = [
            "often-long-ago": .init(count: 4, last: now.addingTimeInterval(-3 * week)),  // 0.5
            "once-today": .init(count: 1, last: now),  // 1
            "twice-today": .init(count: 2, last: now),  // 2
        ]
        let library = ["new", "often-long-ago", "once-today", "older", "twice-today"].map(image)
        #expect(
            Usage.ordered(library, by: entries, now: now).map(\.hash)
                == ["twice-today", "once-today", "often-long-ago", "new", "older"])
    }

    @Test func recordingPersists() {
        let defaults = UserDefaults(suiteName: "kura-usage-\(UUID().uuidString)")!
        Usage(defaults: defaults).record("cat")
        Usage(defaults: defaults).record("cat")
        #expect(Usage(defaults: defaults).entries["cat"]?.count == 2)
    }
}
