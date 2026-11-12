import SwiftUI

/// Rename: a sheet with the current name. Failures show under the field and the
/// sheet stays open; "it was deleted" closes it (the model already said so).
struct RenameSheet: View {
    @Environment(LibraryModel.self) private var library
    @Environment(\.dismiss) private var dismiss
    let image: KuraImage
    @State private var name: String
    @State private var failure: EditFailure?
    @State private var saving = false
    @FocusState private var focused: Bool

    init(image: KuraImage) {
        self.image = image
        _name = State(initialValue: image.name)
    }

    private var trimmed: String { name.trimmingCharacters(in: .whitespacesAndNewlines) }
    private var canSave: Bool { !trimmed.isEmpty && trimmed != image.name && !saving }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Rename Image").font(.headline)
            TextField("Name", text: $name)
                .focused($focused)
                .onSubmit(save)
                .onChange(of: name) { failure = nil }
            if let failure {
                Text(failure.message(for: trimmed))
                    .font(.callout)
                    .foregroundStyle(.red)
            }
            HStack {
                Spacer()
                Button("Cancel", role: .cancel) { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Button("Rename", action: save)
                    .keyboardShortcut(.defaultAction)
                    .buttonStyle(.glassProminent)
                    .disabled(!canSave)
            }
        }
        .padding(20)
        .frame(width: 360)
        .onAppear { focused = true }
    }

    private func save() {
        guard canSave else { return }
        saving = true
        Task {
            failure = await library.rename(image, to: trimmed)
            saving = false
            if failure == nil { dismiss() }
        }
    }
}

/// The glass tag chips under the preview. Each has an × to remove it, and a +
/// turns into a field: Return adds, Esc cancels. Failures show above the bar.
struct TagBar: View {
    @Environment(LibraryModel.self) private var library
    let image: KuraImage
    @State private var adding = false
    @State private var draft = ""
    @State private var failure: String?
    @FocusState private var focused: Bool

    var body: some View {
        VStack(spacing: 6) {
            if let failure {
                Text(failure)
                    .font(.callout)
                    .foregroundStyle(.red)
                    .padding(.horizontal, 12)
                    .padding(.vertical, 6)
                    .glassEffect(.regular, in: .capsule)
            }
            GlassEffectContainer(spacing: 6) {
                HStack(spacing: 6) {
                    Image(systemName: "tag")
                        .foregroundStyle(.secondary)
                        .padding(8)
                        .glassEffect(.regular, in: .circle)
                    if image.tags.isEmpty && !adding {
                        Text("No tags")
                            .foregroundStyle(.secondary)
                            .padding(.horizontal, 12)
                            .padding(.vertical, 6)
                            .glassEffect(.regular, in: .capsule)
                    }
                    ForEach(image.tags, id: \.self) { tag in
                        HStack(spacing: 6) {
                            Text(tag)
                            Button { remove(tag) } label: {
                                Image(systemName: "xmark").font(.caption2.bold())
                            }
                            .buttonStyle(.plain)
                            .foregroundStyle(.secondary)
                            .help("Remove “\(tag)”")
                            .accessibilityLabel("Remove tag \(tag)")
                        }
                        .padding(.leading, 12)
                        .padding(.trailing, 9)
                        .padding(.vertical, 6)
                        .glassEffect(.regular, in: .capsule)
                    }
                    if adding {
                        TextField("Add tag", text: $draft)
                            .textFieldStyle(.plain)
                            .frame(width: 120)
                            .focused($focused)
                            .onSubmit(add)
                            .onExitCommand(perform: cancel)
                            .padding(.horizontal, 12)
                            .padding(.vertical, 6)
                            .glassEffect(.regular, in: .capsule)
                    } else {
                        Button {
                            adding = true
                            focused = true
                        } label: {
                            Image(systemName: "plus").padding(8)
                        }
                        .buttonStyle(.plain)
                        .glassEffect(.regular.interactive(), in: .circle)
                        .help("Add a tag")
                        .accessibilityLabel("Add tag")
                    }
                }
            }
        }
        .onChange(of: focused) {
            if !focused && adding && draft.trimmingCharacters(in: .whitespaces).isEmpty { cancel() }
        }
    }

    private func cancel() {
        adding = false
        draft = ""
        failure = nil
    }

    private func add() {
        let tag = draft.trimmingCharacters(in: .whitespacesAndNewlines)
        if tag.isEmpty { return cancel() }
        guard !tag.contains(",") && !tag.contains(where: \.isNewline) else {
            failure = "A tag can't contain a comma or a line break."
            return
        }
        Task {
            if let result = await library.addTag(image, tag) {
                failure = result.message()
            } else {
                cancel()
            }
        }
    }

    private func remove(_ tag: String) {
        Task {
            failure = await library.removeTag(image, tag)?.message()
        }
    }
}

/// The text in the image, under the preview: what was read, what a person typed,
/// or why there is none yet. The menu edits it by hand or reads it again.
struct CaptionSection: View {
    @Environment(LibraryModel.self) private var library
    let image: KuraImage
    @State private var editing = false
    @State private var failure: String?

    var body: some View {
        VStack(spacing: 4) {
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                content
                Menu {
                    Button("Edit Text…") { editing = true }
                    if image.caption?.isManual == true {
                        Button("Use Read Text") { run { await library.useReadText(image) } }
                    } else if image.caption != nil {
                        Button("Read Text Again") { run { await library.readTextAgain(image) } }
                    }
                } label: {
                    Image(systemName: "text.viewfinder")
                }
                .menuStyle(.borderlessButton)
                .menuIndicator(.hidden)
                .fixedSize()
                .help("Text in this image")
                .accessibilityLabel("Text in this image")
            }
            if let failure {
                Text(failure).font(.caption).foregroundStyle(.red)
            }
        }
        .frame(maxWidth: 560)
        .sheet(isPresented: $editing) { CaptionEditSheet(image: image) }
    }

    @ViewBuilder private var content: some View {
        if let caption = image.caption {
            if caption.text.isEmpty {
                Text(caption.isManual ? "Marked as having no text" : "No text found")
                    .foregroundStyle(.secondary)
            } else {
                ScrollView {
                    Text(caption.text)
                        .textSelection(.enabled)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
                .frame(maxHeight: 72)
                .help(caption.isManual ? "Edited by you" : "Read from the image")
            }
        } else {
            Text(library.captions.progress != nil ? "Reading text…" : "Text not read yet")
                .foregroundStyle(.secondary)
        }
    }

    private func run(_ action: @escaping () async -> EditFailure?) {
        Task {
            failure = await action()?.message()
        }
    }
}

/// Type the text by hand. Searching finds it like read text, and it wins over
/// what was read. Empty means the image has no text.
struct CaptionEditSheet: View {
    @Environment(LibraryModel.self) private var library
    @Environment(\.dismiss) private var dismiss
    let image: KuraImage
    @State private var text: String
    @State private var failure: EditFailure?
    @State private var saving = false

    init(image: KuraImage) {
        self.image = image
        _text = State(initialValue: image.caption?.text ?? "")
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Text in “\(image.name)”").font(.headline)
            Text("Search finds this text. Leave it empty if the image has none.")
                .font(.callout)
                .foregroundStyle(.secondary)
            TextEditor(text: $text)
                .font(.body)
                .frame(minHeight: 120)
                .scrollContentBackground(.hidden)
                .padding(6)
                .background(.quaternary.opacity(0.5), in: .rect(cornerRadius: 8))
            if let failure {
                Text(failure.message()).font(.callout).foregroundStyle(.red)
            }
            HStack {
                Spacer()
                Button("Cancel", role: .cancel) { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Button("Save", action: save)
                    .keyboardShortcut(.defaultAction)
                    .buttonStyle(.glassProminent)
                    .disabled(saving)
            }
        }
        .padding(20)
        .frame(width: 420)
    }

    private func save() {
        saving = true
        Task {
            failure = await library.setCaption(image, to: text)
            saving = false
            if failure == nil { dismiss() }
        }
    }
}
