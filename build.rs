fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default() != "windows" {
        return;
    }

    let version = env!("CARGO_PKG_VERSION");
    let mut parts = version.split('.');
    let major: u64 = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    let minor: u64 = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    let patch: u64 = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    let packed = (major << 48) | (minor << 32) | (patch << 16);
    let version_quad = format!("{version}.0");

    let mut res = winresource::WindowsResource::new();
    // Icon is optional: `packaging/app_icon.ico` is used when present so a
    // missing icon never breaks the release build.
    let icon = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("packaging/app_icon.ico");
    println!("cargo:rerun-if-changed={}", icon.display());
    if icon.is_file() {
        res.set_icon(icon.to_str().expect("icon path must be valid UTF-8"));
    }
    res.set("ProductName", "Keyboard Chatter Fix");
    res.set("FileDescription", "Keyboard Chatter Fix");
    res.set("CompanyName", "twbeatles");
    res.set("InternalName", "KeyboardChatteringFix");
    res.set("OriginalFilename", "KeyboardChatteringFix.exe");
    res.set("LegalCopyright", "Copyright (c) 2026 twbeatles");
    res.set("FileVersion", &version_quad);
    res.set("ProductVersion", &version_quad);
    res.set_version_info(winresource::VersionInfo::FILEVERSION, packed);
    res.set_version_info(winresource::VersionInfo::PRODUCTVERSION, packed);
    // Common Controls v6 (modern buttons/list view, cue banners) and
    // system DPI awareness: the UI scales its layout itself, so text stays
    // crisp instead of being bitmap-stretched on 125%/150% displays.
    res.set_manifest(&format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <assemblyIdentity type="win32" name="twbeatles.KeyboardChatteringFix" version="{version_quad}" processorArchitecture="*"/>
  <dependency>
    <dependentAssembly>
      <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0" processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*"/>
    </dependentAssembly>
  </dependency>
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security><requestedPrivileges><requestedExecutionLevel level="asInvoker" uiAccess="false"/></requestedPrivileges></security>
  </trustInfo>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application><supportedOS Id="{{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}}"/></application>
  </compatibility>
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true</dpiAware>
    </windowsSettings>
  </application>
</assembly>"#
    ));
    res.compile()
        .unwrap_or_else(|err| panic!("failed to embed Windows resources: {err}"));
}
