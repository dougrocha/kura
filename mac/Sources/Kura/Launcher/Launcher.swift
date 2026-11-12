import Observation
import SwiftUI

// A small, opinionated launcher in the style of Raycast. A page supplies
// items and, for each item, its actions; the launcher owns everything else:
// the search field, the highlight, the keyboard, the action panel, the
// footer and the page stack. Every page behaves the same way:
//
//   ↑ ↓ (and ← → in a grid)  move the highlight
//   ↩                        the item's first action (its primary)
//   ⌘↩                       its second action
//   ⌘K                       all its actions, in a panel
//   an action's shortcut     that action, straight away
//   Esc                      close the action panel, else clear the search,
//                            else go back a page, else close the launcher
//
// So a new feature is a page and some actions, never new key handling.

/// One row or tile.
struct LauncherItem: Identifiable, Equatable {
    let id: String
    var title: String
    /// Under the title: why it matched, or its tags.
    var subtitle: AttributedString?
    var image: URL?
}

enum LauncherLayout: Equatable {
    case list
    case grid(columns: Int)
}

/// What the launcher does after an action ran.
enum LauncherOutcome {
    /// Close the launcher, then run `then` (e.g. paste into the app in front).
    case close(then: (@MainActor () -> Void)? = nil)
    /// Stay open, showing `message` in the footer.
    case stay(message: String? = nil)
    /// Open a sub-page; Esc comes back.
    case push(any LauncherPage)
}

struct LauncherShortcut: Equatable, Sendable {
    let key: Character
    let modifiers: EventModifiers

    static func command(_ key: Character, shift: Bool = false) -> Self {
        Self(key: key, modifiers: shift ? [.command, .shift] : .command)
    }

    /// The launcher's own keys, for the footer and the action panel.
    static let primary = Self(key: "\r", modifiers: [])
    static let secondary = Self(key: "\r", modifiers: .command)
    static let actions = command("k")

    var label: String {
        var text = ""
        if modifiers.contains(.control) { text += "⌃" }
        if modifiers.contains(.option) { text += "⌥" }
        if modifiers.contains(.shift) { text += "⇧" }
        if modifiers.contains(.command) { text += "⌘" }
        switch key {
        case "\r": return text + "↩"
        case "\u{7F}", "\u{8}": return text + "⌫"
        default: return text + key.uppercased()
        }
    }
}

struct LauncherAction: Identifiable {
    var id: String { title }
    let title: String
    let systemImage: String
    var shortcut: LauncherShortcut?
    var isDestructive = false
    let perform: @MainActor () async throws -> LauncherOutcome
}

/// A screen of the launcher. Make the class `@Observable` so the view
/// follows `items` and `query`.
@MainActor
protocol LauncherPage: AnyObject {
    /// Shown as a breadcrumb when this page was pushed.
    var title: String { get }
    var placeholder: String { get }
    var query: String { get set }
    var layout: LauncherLayout { get }
    var items: [LauncherItem] { get }
    var emptyTitle: String { get }
    /// First is the primary (↩), second the secondary (⌘↩).
    func actions(for item: LauncherItem) -> [LauncherAction]
    /// The launcher opened onto this page (fresh, or after 30 s away).
    func reset()
}

/// What a key press means. Pure, so the key map is tested and the same
/// everywhere.
enum LauncherCommand: Equatable {
    case move(GridMove)
    case primary
    case secondary
    case toggleActions
    case escape
    case shortcut(LauncherShortcut)
    /// Not ours: let the search field have it.
    case none

    static func from(key: KeyEquivalent, modifiers: EventModifiers, layout: LauncherLayout) -> Self {
        let modifiers = modifiers.intersection([.command, .shift, .option, .control])
        switch key {
        case .upArrow: return .move(.up)
        case .downArrow: return .move(.down)
        // In a list, ← and → stay the search field's (moving the cursor).
        case .leftArrow: return layout == .list ? .none : .move(.left)
        case .rightArrow: return layout == .list ? .none : .move(.right)
        case .escape: return .escape
        case .return: return modifiers.contains(.command) ? .secondary : .primary
        default: break
        }
        guard modifiers.contains(.command) || modifiers.contains(.control) else { return .none }
        let character = Character(key.character.lowercased())
        if character == "k" && modifiers == .command { return .toggleActions }
        return .shortcut(LauncherShortcut(key: character, modifiers: modifiers))
    }
}

@MainActor
@Observable
final class Launcher {
    private(set) var stack: [any LauncherPage]
    private(set) var highlighted = 0
    private(set) var actionsOpen = false
    private(set) var actionHighlighted = 0
    /// Shown in the footer until the next key press ("Copied", an error).
    private(set) var message: String?
    private(set) var isRunning = false
    /// Bumped on every open, so the view refocuses the search field.
    private(set) var opens = 0

    /// Set by the window: hide it.
    @ObservationIgnored var close: () -> Void = {}
    @ObservationIgnored private var closedAt: Date?
    /// Reopening within this long keeps where you were.
    static let memory: TimeInterval = 30

    init(root: any LauncherPage) {
        stack = [root]
    }

    var page: any LauncherPage { stack[stack.count - 1] }

    var query: String {
        get { page.query }
        set {
            page.query = newValue
            highlighted = 0
            message = nil
        }
    }

    var current: LauncherItem? {
        let items = page.items
        return items.isEmpty ? nil : items[min(highlighted, items.count - 1)]
    }

    var actions: [LauncherAction] {
        current.map(page.actions(for:)) ?? []
    }

    /// The launcher became visible.
    func opened(at now: Date = .now) {
        if let closedAt, now.timeIntervalSince(closedAt) < Self.memory {
            // Back within a moment: keep the page, search and highlight.
        } else {
            stack.removeSubrange(1...)
            page.reset()
            highlighted = 0
        }
        actionsOpen = false
        message = nil
        opens += 1
    }

    /// The launcher was hidden (by an action, Esc, or clicking away).
    func closed(at now: Date = .now) {
        closedAt = now
        actionsOpen = false
    }

    /// Returns whether the key was used.
    @discardableResult
    func handle(_ command: LauncherCommand) -> Bool {
        if command != .none { message = nil }
        switch command {
        case .none:
            return false
        case .move(let move):
            if actionsOpen {
                if move == .up || move == .down {
                    let step = move == .up ? -1 : 1
                    actionHighlighted = min(max(actionHighlighted + step, 0), max(actions.count - 1, 0))
                }
            } else {
                let columns = if case .grid(let columns) = page.layout { columns } else { 1 }
                highlighted = GridMove.apply(move, from: highlighted, count: page.items.count, columns: columns)
            }
        case .primary:
            run(actionsOpen ? actionHighlighted : 0)
        case .secondary:
            run(1)
        case .toggleActions:
            actionsOpen = current != nil && !actionsOpen
            actionHighlighted = 0
        case .escape:
            if actionsOpen {
                actionsOpen = false
            } else if !page.query.isEmpty {
                query = ""
            } else if stack.count > 1 {
                stack.removeLast()
                highlighted = 0
            } else {
                close()
            }
        case .shortcut(let shortcut):
            guard let index = actions.firstIndex(where: { $0.shortcut == shortcut }) else { return false }
            run(index)
        }
        return true
    }

    /// Move the highlight straight to an item (a click).
    func highlight(_ index: Int) {
        highlighted = min(max(index, 0), max(page.items.count - 1, 0))
        actionsOpen = false
    }

    func run(_ index: Int) {
        let actions = actions
        guard actions.indices.contains(index), !isRunning else { return }
        let action = actions[index]
        actionsOpen = false
        isRunning = true
        Task {
            defer { isRunning = false }
            do {
                switch try await action.perform() {
                case .close(let then):
                    close()
                    then?()
                case .stay(let message):
                    self.message = message
                case .push(let next):
                    next.reset()
                    stack.append(next)
                    highlighted = 0
                }
            } catch {
                message = "\(action.title) failed: \(error.localizedDescription)"
            }
        }
    }
}

/// Moving a highlight around a grid that fills row by row, like Raycast's.
enum GridMove {
    case left, right, up, down

    /// The new index, or the same one at an edge. Down from the last full row
    /// lands on the last item, so you can always reach it.
    static func apply(_ move: GridMove, from index: Int, count: Int, columns: Int) -> Int {
        guard count > 0 else { return 0 }
        let index = min(max(index, 0), count - 1)
        switch move {
        case .left: return max(index - 1, 0)
        case .right: return min(index + 1, count - 1)
        case .up: return index - columns >= 0 ? index - columns : index
        case .down:
            if index + columns < count { return index + columns }
            // The row below exists but is shorter: go to its last item.
            return (index / columns) < ((count - 1) / columns) ? count - 1 : index
        }
    }
}
