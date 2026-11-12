import SwiftUI

/// The TUI's layout: images on the left, the selected one's preview on the
/// right with its tags underneath. Liquid Glass comes from the system
/// containers (sidebar, toolbar) plus glass tag chips over the preview.
struct ContentView: View {
    @Environment(LibraryModel.self) private var library
    @FocusState private var searchFocused: Bool
    @State private var choosingFiles = false
    @State private var dropTargeted = false
    @State private var renaming: KuraImage?
    @State private var deleting: KuraImage?

    var body: some View {
        @Bindable var library = library
        NavigationSplitView {
            List(library.visibleImages, selection: $library.selection) { image in
                ImageRow(image: image)
            }
            // Return or double-click: copy and hide, like `kura pick`.
            // Act on the rows SwiftUI passes: a right-clicked row needn't be selected.
            .contextMenu(forSelectionType: KuraImage.ID.self) { ids in
                let image = library.image(in: ids)
                Button("Copy") { Task { await library.copy(image) } }
                Divider()
                Button("Rename…") { renaming = image }
                    .disabled(image == nil)
                Button("Delete…", role: .destructive) { deleting = image }
                    .disabled(image == nil)
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
            // ⌘V adds what's on the clipboard: an image, a file from Finder, or a link.
            .onPasteCommand(of: NewImage.acceptedTypes) { providers in
                add(providers)
            }
            .overlay {
                if let message = library.searchError {
                    ContentUnavailableView("Can't Search", systemImage: "exclamationmark.triangle", description: Text(message))
                        .background(.background)
                } else if !library.searchText.isEmpty && library.visibleImages.isEmpty {
                    ContentUnavailableView.search(text: library.searchText)
                }
            }
            // `/` jumps to the search field, as in the TUI.
            .onKeyPress("/") {
                library.focusSearch()
                return .handled
            }
            .searchable(text: $library.searchText, placement: .sidebar, prompt: "Name, tags, text in image, #tag")
            .searchFocused($searchFocused)
            // Return in the search field picks the best match, like `kura pick`.
            .onSubmit(of: .search) { Task { await library.pickBestMatch() } }
            .onChange(of: library.searchRequests) { searchFocused = true }
            .onChange(of: searchFocused) { library.isSearching = searchFocused }
            .navigationSplitViewColumnWidth(min: 220, ideal: 280)
        } detail: {
            ZStack {
                if let image = library.selected {
                    ImageDetail(image: image)
                } else if library.error == nil && !library.isStarting {
                    ContentUnavailableView(
                        library.images.isEmpty ? "No Images" : "No Image Selected",
                        systemImage: "photo.on.rectangle.angled",
                        description: Text(library.images.isEmpty ? "Drop images here, paste one, or press ⌘O." : "")
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
        .onDrop(of: NewImage.acceptedTypes, isTargeted: $dropTargeted) { providers in
            add(providers)
            return true
        }
        .overlay {
            if dropTargeted {
                RoundedRectangle(cornerRadius: 14)
                    .strokeBorder(Color.accentColor, lineWidth: 3)
                    .background(Color.accentColor.opacity(0.08), in: .rect(cornerRadius: 14))
                    .padding(6)
                    .allowsHitTesting(false)
            }
        }
        .fileImporter(isPresented: $choosingFiles, allowedContentTypes: [.image, .movie], allowsMultipleSelection: true) { result in
            guard case .success(let urls) = result else { return }
            Task { await library.add(urls.map(NewImage.file)) }
        }
        .onChange(of: library.addRequests) { choosingFiles = true }
        .onChange(of: library.renameRequests) {
            if renaming == nil && deleting == nil { renaming = library.selected }
        }
        .onChange(of: library.deleteRequests) {
            if renaming == nil && deleting == nil { deleting = library.selected }
        }
        .sheet(item: $renaming) { image in RenameSheet(image: image) }
        .confirmationDialog(
            "Delete “\(deleting?.name ?? "")”?",
            isPresented: Binding(get: { deleting != nil }, set: { if !$0 { deleting = nil } }),
            titleVisibility: .visible, presenting: deleting
        ) { image in
            Button("Delete", role: .destructive) { Task { await library.delete(image) } }
            Button("Cancel", role: .cancel) {}
        } message: { _ in
            Text("This removes the image and its tags from your library. This can't be undone.")
        }
        .overlay(alignment: .bottomTrailing) { UpdatePill() }
        .navigationTitle("Kura")
        .navigationSubtitle(subtitle)
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                Button {
                    choosingFiles = true
                } label: {
                    Label("Add Images", systemImage: "plus")
                }
                .help("Add images (⌘O). You can also drop or paste them.")
                .disabled(library.client == nil)
            }
            ToolbarItem(placement: .primaryAction) {
                Button {
                    Task { await library.copy() }
                } label: {
                    Label("Copy", systemImage: "doc.on.doc")
                }
                .help("Copy the image (⌘C). Return copies and hides Kura.")
                .disabled(library.selected == nil)
            }
        }
        .task { await library.run() }
    }

    private func add(_ providers: [NSItemProvider]) {
        Task { await library.add(await NewImage.load(providers)) }
    }

    private var subtitle: String {
        if library.isStarting { return "Starting…" }
        if library.pendingAdds > 0 { return "Adding \(library.pendingAdds)…" }
        if let progress = library.captions.progress {
            return "Reading text… \(progress.done.formatted()) of \(progress.total.formatted())"
        }
        let total = library.images.count
        let shown = library.visibleImages.count
        var count = library.searchText.isEmpty ? "\(total) images" : "\(shown) of \(total) images"
        if !library.searchText.isEmpty && library.unreadCount > 0 { count += " · \(library.unreadCount) not read yet" }
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
            Thumbnail(url: library.client?.thumbnailURL(image, size: "grid"), cornerRadius: 8)
                .frame(width: 40, height: 40)

            VStack(alignment: .leading, spacing: 2) {
                Text(image.name).lineLimit(1)
                if let subtitle = image.subtitle(library.snippets[image.id]) {
                    Text(subtitle)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }
            }
        }
        .padding(.vertical, 3)
    }
}

/// A square-cropped thumbnail, grey while it loads.
struct Thumbnail: View {
    let url: URL?
    let cornerRadius: CGFloat

    var body: some View {
        AsyncImage(url: url) { image in
            image.resizable().scaledToFill()
        } placeholder: {
            Color.secondary.opacity(0.15)
        }
        .clipShape(.rect(cornerRadius: cornerRadius))
    }
}

struct ImageDetail: View {
    @Environment(LibraryModel.self) private var library
    let image: KuraImage
    /// The original file of an animation, once downloaded: thumbnails are
    /// stills, so GIFs play from the file itself.
    @State private var animation: (hash: String, data: Data)?

    var body: some View {
        VStack(spacing: 12) {
            Group {
                if let animation, animation.hash == image.hash {
                    AnimatedImage(data: animation.data)
                } else {
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
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .task(id: image.hash) {
                guard image.isAnimatable, let client = library.client,
                    let data = try? await client.file(image), !Task.isCancelled
                else { return }
                animation = (image.hash, data)
            }

            CaptionSection(image: image).id(image.hash)
        }
        .padding(24)
        .padding(.bottom, 44)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        // The tags float over the preview on glass, like the TUI's tag bar.
        .overlay(alignment: .bottom) {
            TagBar(image: image).id(image.hash).padding(14)
        }
        .navigationTitle(image.name)
    }
}

extension KuraImage {
    /// Formats that can move. A still WebP plays as a still, which is fine.
    var isAnimatable: Bool { mimeType == "image/gif" || mimeType == "image/webp" }
}

/// Plays an animated image (AppKit animates GIFs and WebPs; SwiftUI's Image
/// shows only the first frame), scaled to fit like `scaledToFit`.
struct AnimatedImage: NSViewRepresentable {
    let data: Data

    func makeNSView(context: Context) -> NSImageView {
        let view = NSImageView()
        view.imageScaling = .scaleProportionallyUpOrDown
        view.animates = true
        view.isEditable = false
        // Let SwiftUI size it; the image's own size mustn't push the layout.
        for axis in [NSLayoutConstraint.Orientation.horizontal, .vertical] {
            view.setContentCompressionResistancePriority(.defaultLow, for: axis)
            view.setContentHuggingPriority(.defaultLow, for: axis)
        }
        return view
    }

    /// Only on new data: setting the image again would restart the animation
    /// and decode every frame again on each re-render.
    func updateNSView(_ view: NSImageView, context: Context) {
        guard context.coordinator.data != data else { return }
        context.coordinator.data = data
        view.image = NSImage(data: data)
    }

    func makeCoordinator() -> Coordinator { Coordinator() }

    final class Coordinator {
        var data: Data?
    }
}
