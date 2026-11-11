import SwiftUI

/// The TUI's layout: images on the left, the selected one's preview on the
/// right with its tags underneath. Liquid Glass comes from the system
/// containers (sidebar, toolbar) plus glass tag chips over the preview.
struct ContentView: View {
    @Environment(LibraryModel.self) private var library
    @FocusState private var searchFocused: Bool

    var body: some View {
        @Bindable var library = library
        NavigationSplitView {
            List(library.visibleImages, selection: $library.selection) { image in
                ImageRow(image: image)
            }
            // Return or double-click: copy and hide, like `kura pick`.
            // Act on the rows SwiftUI passes: a right-clicked row needn't be selected.
            .contextMenu(forSelectionType: KuraImage.ID.self) { ids in
                Button("Copy") { Task { await library.copy(library.image(in: ids)) } }
            } primaryAction: { ids in
                Task { await library.pick(library.image(in: ids)) }
            }
            // ⌘C while the list has focus; the search field keeps its own ⌘C.
            // The copy downloads the file, so it can't hand SwiftUI providers
            // synchronously. SwiftUI may clear the clipboard for the empty
            // answer, so a failed copy restores what was there.
            .onCopyCommand {
                let snapshot = Pasteboard.snapshot()
                Task { await library.copy(restoring: snapshot) }
                return []
            }
            .overlay {
                if !library.searchText.isEmpty && library.visibleImages.isEmpty {
                    ContentUnavailableView.search(text: library.searchText)
                }
            }
            // `/` jumps to the search field, as in the TUI.
            .onKeyPress("/") {
                library.focusSearch()
                return .handled
            }
            .searchable(text: $library.searchText, placement: .sidebar, prompt: "Name, tags, #tag")
            .searchFocused($searchFocused)
            // Return in the search field picks the best match, like `kura pick`.
            .onSubmit(of: .search) { Task { await library.pick() } }
            .onChange(of: library.searchRequests) { searchFocused = true }
            .navigationSplitViewColumnWidth(min: 220, ideal: 280)
        } detail: {
            ZStack {
                if let image = library.selected {
                    ImageDetail(image: image)
                } else if library.error == nil && !library.isStarting {
                    ContentUnavailableView(
                        library.images.isEmpty ? "No Images" : "No Image Selected",
                        systemImage: "photo.on.rectangle.angled",
                        description: Text(library.images.isEmpty ? "Add some with `kura add <file>`." : "")
                    )
                }
            }
            .overlay { status }
            .overlay(alignment: .top) {
                if let notice = library.notice {
                    Text(notice)
                        .padding(.horizontal, 14)
                        .padding(.vertical, 8)
                        .glassEffect(.regular, in: .capsule)
                        .padding(12)
                        .transition(.move(edge: .top).combined(with: .opacity))
                }
            }
            .animation(.snappy, value: library.notice)
        }
        .navigationTitle("Kura")
        .navigationSubtitle(subtitle)
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                Button {
                    Task { await library.copy() }
                } label: {
                    Label("Copy", systemImage: "doc.on.doc")
                }
                .help("Copy the image (⌘C). Return copies and hides Kura.")
                .disabled(library.selected == nil)
            }
            ToolbarItem(placement: .primaryAction) {
                Button {
                    Task { await library.load() }
                } label: {
                    Label("Reload", systemImage: "arrow.clockwise")
                }
                .disabled(library.client == nil || library.isLoading)
            }
        }
        .task { await library.run() }
    }

    private var subtitle: String {
        if library.isStarting { return "Starting…" }
        let total = library.images.count
        let shown = library.visibleImages.count
        let count = library.searchText.isEmpty ? "\(total) images" : "\(shown) of \(total) images"
        return library.isLive ? "\(count) · Live" : count
    }

    @ViewBuilder private var status: some View {
        if let error = library.error {
            ContentUnavailableView {
                Label("Kura Isn't Available", systemImage: "exclamationmark.triangle")
            } description: {
                Text(error)
            } actions: {
                Button("Try Again") { Task { await library.retry() } }
                    .buttonStyle(.glassProminent)
            }
            .background(.background)
        } else if library.isStarting {
            ProgressView("Opening your library…")
        }
    }
}

struct ImageRow: View {
    @Environment(LibraryModel.self) private var library
    let image: KuraImage

    var body: some View {
        HStack(spacing: 10) {
            AsyncImage(url: library.client?.thumbnailURL(image, size: "grid")) { thumb in
                thumb.resizable().scaledToFill()
            } placeholder: {
                Color.secondary.opacity(0.15)
            }
            .frame(width: 40, height: 40)
            .clipShape(.rect(cornerRadius: 8))

            VStack(alignment: .leading, spacing: 2) {
                Text(image.name).lineLimit(1)
                if !image.tags.isEmpty {
                    Text(image.tags.joined(separator: ", "))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }
            }
        }
        .padding(.vertical, 3)
    }
}

struct ImageDetail: View {
    @Environment(LibraryModel.self) private var library
    let image: KuraImage

    var body: some View {
        AsyncImage(url: library.client?.thumbnailURL(image, size: "preview")) { phase in
            switch phase {
            case .success(let preview):
                preview.resizable().scaledToFit()
            case .failure:
                ContentUnavailableView("Can't Load Preview", systemImage: "exclamationmark.triangle")
            default:
                ProgressView()
            }
        }
        .padding(24)
        .padding(.bottom, 44)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        // The tags float over the preview on glass, like the TUI's tag bar.
        .overlay(alignment: .bottom) {
            TagBar(tags: image.tags).padding(14)
        }
        .navigationTitle(image.name)
    }
}

struct TagBar: View {
    let tags: [String]

    var body: some View {
        GlassEffectContainer(spacing: 6) {
            HStack(spacing: 6) {
                Image(systemName: "tag")
                    .foregroundStyle(.secondary)
                    .padding(8)
                    .glassEffect(.regular, in: .circle)
                if tags.isEmpty {
                    Text("No tags")
                        .foregroundStyle(.secondary)
                        .padding(.horizontal, 12)
                        .padding(.vertical, 6)
                        .glassEffect(.regular, in: .capsule)
                } else {
                    ForEach(tags, id: \.self) { tag in
                        Text(tag)
                            .padding(.horizontal, 12)
                            .padding(.vertical, 6)
                            .glassEffect(.regular, in: .capsule)
                    }
                }
            }
        }
    }
}
