import Carbon.HIToolbox

/// A system-wide keyboard shortcut that works while another app is in front.
///
/// Carbon's `RegisterEventHotKey` is still how macOS does this: it needs no
/// Accessibility permission (the system delivers only this one key, not a
/// keystroke feed), unlike an `NSEvent` global monitor.
@MainActor
final class HotKey {
    /// ⌃⌥K: "K for Kura", and rarely taken by other apps.
    static let quickPick = (keyCode: UInt32(kVK_ANSI_K), modifiers: UInt32(controlKey | optionKey), label: "⌃⌥K")

    private var hotKey: EventHotKeyRef?
    private static var handler: EventHandlerRef?
    private static var actions: [UInt32: @MainActor () -> Void] = [:]
    private static var nextID: UInt32 = 1
    private let id: UInt32

    /// Registers the shortcut; nil if the system refused it (another app owns it).
    init?(keyCode: UInt32, modifiers: UInt32, action: @escaping @MainActor () -> Void) {
        Self.installHandler()
        id = Self.nextID
        Self.nextID += 1
        let hotKeyID = EventHotKeyID(signature: OSType(0x4B55_5241), id: id)  // 'KURA'
        guard RegisterEventHotKey(keyCode, modifiers, hotKeyID, GetApplicationEventTarget(), 0, &hotKey) == noErr
        else { return nil }
        Self.actions[id] = action
    }

    isolated deinit {
        if let hotKey { UnregisterEventHotKey(hotKey) }
        Self.actions[id] = nil
    }

    /// One handler for every hot key of the app; it looks up which one fired.
    private static func installHandler() {
        guard handler == nil else { return }
        var spec = EventTypeSpec(eventClass: OSType(kEventClassKeyboard), eventKind: UInt32(kEventHotKeyPressed))
        InstallEventHandler(
            GetApplicationEventTarget(),
            { _, event, _ in
                var hotKeyID = EventHotKeyID()
                let status = GetEventParameter(
                    event, EventParamName(kEventParamDirectObject), EventParamType(typeEventHotKeyID), nil,
                    MemoryLayout<EventHotKeyID>.size, nil, &hotKeyID)
                guard status == noErr else { return status }
                let id = hotKeyID.id
                // The application event target runs its handlers on the main thread.
                MainActor.assumeIsolated { HotKey.actions[id]?() }
                return noErr
            }, 1, &spec, nil, &handler)
    }
}
