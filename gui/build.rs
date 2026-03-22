fn main() {
    // Embed Windows application icon and metadata into the .exe binary.
    // This gives the .exe its icon in the taskbar, desktop shortcuts, and file explorer.
    #[cfg(windows)]
    {
        let mut res = tauri_winres::WindowsResource::new();
        res.set_icon("../assets/icon.ico");
        res.set("ProductName", "Asset Tap");
        res.set("FileDescription", "Asset Tap");
        res.compile().expect("Failed to compile Windows resources");
    }
}
