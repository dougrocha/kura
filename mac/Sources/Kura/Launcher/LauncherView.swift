import SwiftUI

/// Draws a `Launcher`: search field, results, footer, action panel. Holds no
/// state of its own except focus; every key goes through `LauncherCommand`.
struct LauncherView: View {
    static let size = CGSize(width: 720, height: 520)
    let launcher: Launcher
    @FocusState private var focused: Bool

    var body: some View {
        @Bindable var launcher = launcher
        VStack(spacing: 0) {
            HStack(spacing: 10) {
                if launcher.stack.count > 1 {
                    Text(launcher.page.title)
                        .font(.callout.weight(.medium))
                        .padding(.horizontal, 8)
                        .padding(.vertical, 3)
                        .background(.quaternary, in: .capsule)
                } else {
                    Image(systemName: "magnifyingglass").font(.title2).foregroundStyle(.secondary)
                }
                TextField(launcher.page.placeholder, text: $launcher.query)
                    .textFieldStyle(.plain)
                    .font(.title2)
                    .focused($focused)
                    .onKeyPress(phases: [.down, .repeat]) { press in
                        let command = LauncherCommand.from(
                            key: press.key, modifiers: press.modifiers, layout: launcher.page.layout)
                        return launcher.handle(command) ? .handled : .ignored
                    }
            }
            .padding(16)

            Divider()

            LauncherResults(launcher: launcher)
                .frame(maxHeight: .infinity)

            Divider()
            LauncherFooter(launcher: launcher)
        }
        .frame(width: Self.size.width, height: Self.size.height)
        .glassEffect(.regular, in: .rect(cornerRadius: 18))
        .overlay(alignment: .bottomTrailing) {
            if launcher.actionsOpen {
                LauncherActionPanel(launcher: launcher)
                    .padding(.trailing, 12)
                    .padding(.bottom, 44)
                    .transition(.opacity.combined(with: .scale(scale: 0.96, anchor: .bottomTrailing)))
            }
        }
        .animation(.snappy(duration: 0.15), value: launcher.actionsOpen)
        .onAppear { focused = true }
        .onChange(of: launcher.opens) { focused = true }
    }
}

private struct LauncherResults: View {
    let launcher: Launcher

    var body: some View {
        let items = launcher.page.items
        if items.isEmpty {
            ContentUnavailableView(launcher.page.emptyTitle, systemImage: "photo.on.rectangle.angled")
        } else {
            ScrollViewReader { scroller in
                ScrollView {
                    switch launcher.page.layout {
                    case .list:
                        LazyVStack(spacing: 2) {
                            ForEach(Array(items.enumerated()), id: \.element.id) { offset, item in
                                LauncherRow(item: item, highlighted: offset == launcher.highlighted)
                                    .id(item.id)
                                    .contentShape(.rect)
                                    .onTapGesture { select(offset) }
                            }
                        }
                        .padding(8)
                    case .grid(let columns):
                        LazyVGrid(columns: Array(repeating: GridItem(.flexible(), spacing: 10), count: columns), spacing: 10) {
                            ForEach(Array(items.enumerated()), id: \.element.id) { offset, item in
                                LauncherTile(item: item, highlighted: offset == launcher.highlighted)
                                    .id(item.id)
                                    .contentShape(.rect)
                                    .onTapGesture { select(offset) }
                            }
                        }
                        .padding(12)
                    }
                }
                .onChange(of: launcher.highlighted) {
                    if let id = launcher.current?.id { scroller.scrollTo(id) }
                }
            }
        }
    }

    /// A click runs the primary action, like Return.
    private func select(_ offset: Int) {
        launcher.highlight(offset)
        launcher.handle(.primary)
    }
}

private struct LauncherRow: View {
    let item: LauncherItem
    let highlighted: Bool

    var body: some View {
        HStack(spacing: 12) {
            Thumbnail(url: item.image, cornerRadius: 10).frame(width: 44, height: 44)
            VStack(alignment: .leading, spacing: 2) {
                Text(item.title).font(.headline).lineLimit(1)
                if let subtitle = item.subtitle {
                    Text(subtitle).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                }
            }
            Spacer(minLength: 0)
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 6)
        .background(highlighted ? Color.accentColor.opacity(0.25) : .clear, in: .rect(cornerRadius: 10))
    }
}

private struct LauncherTile: View {
    let item: LauncherItem
    let highlighted: Bool

    var body: some View {
        VStack(spacing: 6) {
            Thumbnail(url: item.image, cornerRadius: 10)
                .aspectRatio(1, contentMode: .fit)
                .overlay {
                    RoundedRectangle(cornerRadius: 10)
                        .strokeBorder(highlighted ? Color.accentColor : .clear, lineWidth: 3)
                }
            Text(item.title)
                .font(.caption.weight(highlighted ? .semibold : .regular))
                .lineLimit(1)
                .foregroundStyle(highlighted ? .primary : .secondary)
        }
    }
}

private struct LauncherFooter: View {
    let launcher: Launcher

    var body: some View {
        HStack(spacing: 14) {
            if let message = launcher.message {
                Text(message).foregroundStyle(.secondary).lineLimit(1)
            } else if let current = launcher.current {
                Text(current.title).foregroundStyle(.secondary).lineLimit(1)
            }
            Spacer()
            if let primary = launcher.actions.first {
                KeyHint(title: primary.title, keys: LauncherShortcut.primary.label)
                Divider().frame(height: 14)
                KeyHint(title: "Actions", keys: LauncherShortcut.actions.label)
            }
        }
        .font(.caption)
        .padding(.horizontal, 14)
        .padding(.vertical, 9)
    }
}

private struct KeyHint: View {
    let title: String
    let keys: String

    var body: some View {
        HStack(spacing: 6) {
            Text(title).fontWeight(.medium)
            Text(keys)
                .font(.caption.monospaced())
                .padding(.horizontal, 5)
                .padding(.vertical, 1)
                .background(.quaternary, in: .rect(cornerRadius: 4))
        }
    }
}

private struct LauncherActionPanel: View {
    let launcher: Launcher

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            if let current = launcher.current {
                Text(current.title)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                    .padding(.horizontal, 10)
                    .padding(.top, 6)
                    .padding(.bottom, 2)
            }
            ForEach(Array(launcher.actions.enumerated()), id: \.element.id) { offset, action in
                HStack(spacing: 10) {
                    Image(systemName: action.systemImage).frame(width: 18)
                    Text(action.title)
                    Spacer(minLength: 20)
                    if let label = (offset == 0 ? .primary : offset == 1 ? .secondary : action.shortcut)?.label {
                        Text(label).font(.caption.monospaced()).foregroundStyle(.secondary)
                    }
                }
                .foregroundStyle(action.isDestructive ? .red : .primary)
                .padding(.horizontal, 10)
                .padding(.vertical, 6)
                .background(
                    offset == launcher.actionHighlighted ? Color.accentColor.opacity(0.25) : .clear,
                    in: .rect(cornerRadius: 8)
                )
                .contentShape(.rect)
                .onTapGesture { launcher.run(offset) }
            }
        }
        .padding(6)
        .frame(width: 300)
        .glassEffect(.regular, in: .rect(cornerRadius: 14))
    }
}
