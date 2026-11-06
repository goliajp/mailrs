import SwiftUI

/// What a swipe on a row does, per direction. Saved on the server, so the
/// web and the Android app follow the same choice.
struct SwipeSettingsSection: View {
    @Environment(Session.self) private var session

    var body: some View {
        Section {
            picker("Swipe right", id: "settings.swipe.right", side: \.right)
            picker("Swipe left", id: "settings.swipe.left", side: \.left)
        } header: {
            Text("Swiping a conversation")
        } footer: {
            Text("The same setting is used on the web and in the Android app. In the Trash either swipe restores the conversation")
        }
        .task { await session.loadSwipe() }
    }

    private func picker(
        _ title: LocalizedStringKey, id: String, side: WritableKeyPath<SwipeSetting, SwipeChoice>
    ) -> some View {
        Picker(title, selection: Binding(
            get: { session.swipe[keyPath: side] },
            set: { choice in
                var next = session.swipe
                next[keyPath: side] = choice
                Task { await session.saveSwipe(next) }
            }
        )) {
            ForEach(SwipeChoice.allCases, id: \.self) { choice in
                Text(LocalizedStringKey(choice.titleKey)).tag(choice)
            }
        }
        .accessibilityIdentifier(id)
    }
}
