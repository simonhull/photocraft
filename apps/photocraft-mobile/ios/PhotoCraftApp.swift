import SwiftUI
import UniformTypeIdentifiers

/// PhotoCraft for iPad. CraftKit provides the window: the canvas, Liquid Glass toolbar, the
/// system document picker, exporter and share sheet, and Import from Photos. The shell draws the
/// chrome natively (CraftUI), described by the Rust app: PhotoCraft's menus in the iPad menu bar
/// and a toolbar menu, its tools as a rail, the Layers and Documents inspectors and the brush
/// popover, and the status as toasts and progress. egui draws just the canvas.
@main
struct PhotoCraftApp: App {
    /// Puts PhotoCraft's menus in the iPad menu bar.
    @UIApplicationDelegateAdaptor(CraftMenuDelegate.self) private var menus

    var body: some Scene {
        WindowGroup {
            CraftShell(
                documentTypes: PhotoCraftApp.documentTypes, pencil: PhotoCraftApp.pencil,
                nativePanels: true, menus: true, rail: true, status: true
            )
        }
    }

    /// Apple Pencil's double-tap swaps the foreground and background colours, as X does: on a
    /// mask, that flips the brush between hiding and revealing.
    static let pencil = CraftPencilActions(doubleTap: .command("tools.swapColors"))

    /// What PhotoCraft opens: its own documents, Photoshop documents, then any image.
    static let documentTypes: [UTType] = [UTType("dev.craft.photocraft.document"), UTType("com.adobe.photoshop-image"), .image].compactMap { $0 }
}
