import SwiftUI

/// The TUI's layout: images on the left, the selected one's preview on the
/// right with its tags underneath. Liquid Glass comes from the system
/// containers (sidebar, toolbar) plus glass tag chips over the preview.
struct ContentView: View {
    @Environment(LibraryModel.self) private var library

    var body: some View {
        @Bindable var library = library
        NavigationSplitView {
            List(library.images, selection: $library.selection) { image in
                ImageRow(image: image)
            }
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
        }
        .navigationTitle("Kura")
        .navigationSubtitle(subtitle)
        .toolbar {
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
        let count = "\(library.images.count) images"
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
