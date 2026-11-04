import Foundation

/// What the reader's image setting means for one message on screen.
///
/// `auto` loads remote images with the message; `click` holds them until
/// "Load images" is tapped; `never` holds them and offers no button. The
/// tap is per message and not remembered, whatever the setting.
enum RemoteImages {
    static func blocked(_ setting: SettingsWire.Images, loaded: Bool) -> Bool {
        switch setting {
        case .auto: return false
        case .click: return !loaded
        case .never: return true
        }
    }

    static func offersButton(_ setting: SettingsWire.Images, loaded: Bool) -> Bool {
        setting == .click && !loaded
    }
}
