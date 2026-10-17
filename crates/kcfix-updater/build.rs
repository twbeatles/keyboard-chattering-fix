fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default() != "windows" {
        return;
    }

    let manifest = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="false"/>
      </requestedPrivileges>
    </security>
  </trustInfo>
</assembly>"#;

    let version = env!("CARGO_PKG_VERSION");
    let mut parts = version.split('.');
    let major: u64 = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    let minor: u64 = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    let patch: u64 = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    let packed = (major << 48) | (minor << 32) | (patch << 16);
    let version_quad = format!("{version}.0");

    let mut res = winresource::WindowsResource::new();
    res.set_manifest(manifest);
    res.set("ProductName", "Keyboard Chatter Fix Updater");
    res.set(
        "FileDescription",
        "Self-update helper for Keyboard Chatter Fix",
    );
    res.set("OriginalFilename", "kcfix-updater.exe");
    res.set("FileVersion", &version_quad);
    res.set("ProductVersion", &version_quad);
    res.set_version_info(winresource::VersionInfo::FILEVERSION, packed);
    res.set_version_info(winresource::VersionInfo::PRODUCTVERSION, packed);
    res.compile()
        .unwrap_or_else(|err| panic!("failed to compile updater resources: {err}"));
}
