import SwiftUI

/// The TUI's layout: images on the left, the selected one's preview on the
/// right with its tags underneath.
struct ContentView: View {
    @Environment(LibraryModel.self) private var library

    var body: some View {
        @Bindable var library = library
        NavigationSplitView {
            List(library.images, selection: $library.selection) { image in
                ImageRow(image: image)
            }
            .navigationSplitViewColumnWidth(min: 200, ideal: 260)
            .overlay { sidebarPlaceholder }
        } detail: {
            if let image = library.selected {
                ImageDetail(image: image)
            } else {
                ContentUnavailableView("No Image Selected", systemImage: "photo")
            }
        }
        .navigationTitle("Kura")
        .navigationSubtitle("\(library.images.count) images" + (library.isLive ? " · live" : ""))
        .toolbar {
            Button {
                Task { await library.load() }
            } label: {
                Label("Reload", systemImage: "arrow.clockwise")
            }
            .disabled(library.isLoading)
        }
        .task {
            await library.load()
            await library.follow()
        }
    }

    @ViewBuilder private var sidebarPlaceholder: some View {
        if let error = library.error {
            ContentUnavailableView {
                Label("Can't Reach Kura", systemImage: "bolt.horizontal.circle")
            } description: {
                Text("\(error)\n\nStart the server with `just serve`.")
            } actions: {
                Button("Try Again") { Task { await library.load() } }
            }
        } else if library.images.isEmpty && !library.isLoading {
            ContentUnavailableView("No Images", systemImage: "photo.stack",
                                   description: Text("Add some with `just run add <file>`."))
        }
    }
}

struct ImageRow: View {
    @Environment(LibraryModel.self) private var library
    let image: KuraImage

    var body: some View {
        HStack(spacing: 10) {
            AsyncImage(url: library.client.thumbnailURL(image, size: "grid")) { thumb in
                thumb.resizable().scaledToFill()
            } placeholder: {
                Color.secondary.opacity(0.15)
            }
            .frame(width: 36, height: 36)
            .clipShape(RoundedRectangle(cornerRadius: 6))

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
        .padding(.vertical, 2)
    }
}

struct ImageDetail: View {
    @Environment(LibraryModel.self) private var library
    let image: KuraImage

    var body: some View {
        VStack(spacing: 0) {
            AsyncImage(url: library.client.thumbnailURL(image, size: "preview")) { phase in
                switch phase {
                case .success(let preview):
                    preview.resizable().scaledToFit()
                case .failure:
                    ContentUnavailableView("Can't Load Preview", systemImage: "exclamationmark.triangle")
                default:
                    ProgressView()
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .padding()

            Divider()
            TagBar(tags: image.tags)
        }
        .navigationTitle(image.name)
    }
}

struct TagBar: View {
    let tags: [String]

    var body: some View {
        HStack {
            Label("Tags", systemImage: "tag").foregroundStyle(.secondary)
            if tags.isEmpty {
                Text("none").foregroundStyle(.tertiary)
            } else {
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 6) {
                        ForEach(tags, id: \.self) { tag in
                            Text(tag)
                                .padding(.horizontal, 8)
                                .padding(.vertical, 3)
                                .background(.quaternary, in: Capsule())
                        }
                    }
                }
            }
            Spacer(minLength: 0)
        }
        .padding(.horizontal, 12)
        .frame(height: 40)
    }
}
