---
title: "Distributing Rust desktop applications: codesigning and packaging"
description: "How to package and distribute Rust desktop apps: codesigning, notarization, DMG creation, and auto-updates."
date: 2026-06-05
tags: [Rust, distribution, guide]
draft: false
---

You wrote a Rust desktop app. It compiles. It runs on your machine. Now you need to get it onto other people's machines, and that is where things get messy. Codesigning certificates, notarization workflows, DMG installers, Windows MSIX packages, Linux AppImages, and auto-update mechanisms. Each platform has its own tooling, its own certificate requirements, and its own ways to reject your build.

I have shipped Rust desktop apps across all three platforms. Here is what I learned, with the specific tools and commands I use.

## Why codesigning matters

Unsigned applications trigger OS-level warnings. On macOS, users see "cannot be opened because the developer cannot be verified." On Windows, SmartScreen blocks the download. On Linux, AppImages run fine without signatures, but Flatpak and Snap require them for their stores.

Codesigning solves this. A valid signature tells the OS that a known developer produced the binary and that it has not been tampered with. Without it, your download page turns into a support channel.

The cost is real. An Apple Developer account runs $99/year. A Windows code signing certificate from a CA like DigiCert or Sectigo costs $200-500/year depending on the validation level. Linux is free if you stick to AppImages and skip the stores. Budget accordingly.

## Codesigning on macOS

macOS has the strictest requirements. You need an Apple Developer certificate, and starting with macOS Ventura, you also need to notarize the app through Apple's servers. The full pipeline looks like this: build the binary, embed it in an app bundle, sign the bundle, notarize it, and staple the notarization ticket.

First, build a release binary and package it into an `.app` bundle. The bundle structure is rigid:

```
MyApp.app/
  Contents/
    Info.plist
    MacOS/
      my-app          # your compiled binary
    Resources/
      icon.icns
    _CodeSignature/
```

The `Info.plist` must include `CFBundleIdentifier`, `CFBundleVersion`, and `CFBundleShortVersionString`. Without these, `codesign` will fail.

Sign the bundle with your Developer ID Application certificate:

```bash
# List your signing identities
security find-identity -v -p codesigning

# Sign the app
codesign --force --deep --sign "Developer ID Application: Your Name (TEAMID)" \
  --options runtime \
  --entitlements entitlements.plist \
  target/release/bundle/osx/MyApp.app
```

The `--options runtime` flag enables the Hardened Runtime, which is required for notarization. Your `entitlements.plist` declares any exceptions you need, like network access or camera permissions.

### Notarization

Notarization sends your app to Apple's servers for automated malware scanning. It takes 1-10 minutes. You need an app-specific password from your Apple ID account page.

```bash
# Create a ZIP for notarization
ditto -c -k --keepParent MyApp.app MyApp.zip

# Submit for notarization
xcrun notarytool submit MyApp.zip \
  --apple-id "you@example.com" \
  --team-id "TEAMID" \
  --password "xxxx-xxxx-xxxx-xxxx" \
  --wait

# Staple the notarization ticket to the app
xcrun stapler staple MyApp.app
```

The `--wait` flag blocks until notarization completes. If it fails, `notarytool log` gives you the specific reason. Common failures include missing `Info.plist` keys, unsigned dynamic libraries, and paths with spaces that confuse the notarization tool.

## Codesigning on Windows

Windows uses Authenticode signatures. You buy a code signing certificate from a CA, then use `signtool` (part of the Windows SDK) to sign your binary.

```powershell
signtool sign /fd SHA256 /a /tr http://timestamp.digicert.com /td SHA256 ^
  /f my-cert.pfx /p "cert-password" ^
  target\release\my-app.exe
```

The `/tr` flag specifies a timestamp server. Without a timestamp, your signature becomes invalid the day your certificate expires. With a timestamp, signatures remain valid forever.

For EV (Extended Validation) certificates, you will likely use a hardware token. The signing command is the same, but `signtool` reads the key from the USB device instead of a PFX file.

### Creating a Windows installer

WiX Toolset generates MSI installers from XML definitions. The setup is verbose but gives you full control over install location, shortcuts, and registry entries.

```xml
<Product Id="*" Name="MyApp" Language="1033"
         Version="1.0.0" Manufacturer="Your Name"
         UpgradeCode="YOUR-GUID-HERE">
  <Package InstallerVersion="200" Compressed="yes" />
  <Media Id="1" Cabinet="myapp.cab" EmbedCab="yes" />
  <Directory Id="TARGETDIR" Name="SourceDir">
    <Directory Id="ProgramFilesFolder">
      <Directory Id="INSTALLFOLDER" Name="MyApp" />
    </Directory>
  </Directory>
</Product>
```

NSIS is another option. It uses a scripting language instead of XML and produces smaller installers. I prefer WiX because the XML schema is well-documented and the MSI format integrates cleanly with enterprise deployment tools.

## Packaging for Linux

Linux has no single packaging format. AppImage is the simplest because it produces a single portable file. Flatpak and Snap require sandboxing metadata and store-specific manifests.

For an AppImage, use `cargo-bundle` or build manually:

```bash
# Build a release binary for Linux
cargo build --release

# Package with linuxdeploy
wget https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-x86_64.AppImage
chmod +x linuxdeploy-x86_64.AppImage

./linuxdeploy-x86_64.AppImage \
  --executable=target/release/my-app \
  --desktop-file=my-app.desktop \
  --icon-file=icon.png \
  --output=appimage
```

The `.desktop` file defines how the app appears in the system launcher. It specifies the app name, icon, categories, and executable path. Without it, desktop environments will not show your app in their menus.

## Creating a DMG for macOS

A DMG is the standard macOS distribution format. It presents a drag-to-Applications installer that Mac users expect. You can create one with `hdiutil`:

```bash
# Create a temporary folder with the app and a symlink to /Applications
mkdir -p dmg-temp
cp -r MyApp.app dmg-temp/
ln -s /Applications dmg-temp/Applications

# Create the DMG
hdiutil create -volname "MyApp" \
  -srcfolder dmg-temp \
  -ov -format UDZO \
  MyApp.dmg

# Clean up
rm -rf dmg-temp
```

For a DMG with a custom background and icon positioning, you need to use `create-dmg` or AppleScript to set the Finder window properties. The bare `hdiutil` approach works fine for developer tools where users do not care about polish.

## Auto-updates

Once your app is on someone's machine, you need a way to push updates. The architecture is straightforward: the app periodically checks a known URL for a JSON manifest, compares versions, and downloads the new binary if available. The hard part is doing this safely.

### The manifest

A simple update manifest looks like this:

```json
{
  "version": "1.2.0",
  "release_date": "2026-06-01",
  "platforms": {
    "darwin-aarch64": {
      "url": "https://releases.example.com/myapp/1.2.0/MyApp.dmg",
      "size": 8432000,
      "sha256": "e3b0c44298fc1c149afbf4c8..."
    },
    "windows-x86_64": {
      "url": "https://releases.example.com/myapp/1.2.0/MyApp-setup.exe",
      "size": 6200000,
      "sha256": "ba7816bf8f01cfea4141..."
    }
  }
}
```

The SHA-256 hash lets the client verify the download completed without corruption. But hashes alone do not protect against a compromised server. An attacker who controls your release server can replace the binary and update the hash.

### Signing the manifest

Ed25519 signing solves this. You embed the public key in your app at compile time and sign the manifest with the private key during your release process. The app verifies the signature before trusting any URL or hash in the manifest.

```rust
use ed25519_dalek::{SigningKey, VerifyingKey, Signer, Verifier};
use ed25519_dalek::Signature;

/// The public key baked into the app at compile time.
const UPDATE_PUBLIC_KEY: &[u8; 32] = include_bytes!("../update_pub.key");

pub fn verify_update_manifest(
    manifest_bytes: &[u8],
    signature_bytes: &[u8; 64],
) -> bool {
    let verifying_key = match VerifyingKey::from_bytes(UPDATE_PUBLIC_KEY) {
        Ok(key) => key,
        Err(_) => return false,
    };
    let signature = Signature::from_bytes(signature_bytes);
    verifying_key.verify(manifest_bytes, &signature).is_ok()
}
```

The private key never leaves your build machine. If someone compromises your release server, they cannot produce a valid signature, so the app rejects the malicious update. This is the same model Tailscale and other security-focused tools use.

For more on building the auto-update system itself, including how to structure the background checker and handle platform-specific install flows, see the [getting started guide](/docs/docs/getting-started/).

## CI pipeline for releases

GitHub Actions can automate the entire pipeline. Build on three runners, sign with platform-specific tools, and upload to a release.

```yaml
# Simplified release workflow
jobs:
  build-macos:
    runs-on: macos-latest
    steps:
      - uses: actions/checkout@v4
      - run: cargo build --release
      - run: |
          codesign --force --deep --sign "$DEVELOPER_ID" \
            --options runtime target/release/bundle/osx/MyApp.app
      - run: |
          xcrun notarytool submit MyApp.zip \
            --apple-id "$APPLE_ID" \
            --team-id "$TEAM_ID" \
            --password "$NOTARY_PASSWORD" \
            --wait

  build-windows:
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@v4
      - run: cargo build --release
      - run: |
          signtool sign /fd SHA256 /tr http://timestamp.digicert.com ^
            /f cert.pfx /p "$CERT_PASSWORD" target/release/my-app.exe

  build-linux:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: cargo build --release
      - run: ./linuxdeploy --output=appimage
```

Store certificates and passwords as GitHub Actions secrets. Never check them into the repository. The macOS signing certificate should be stored as a base64-encoded PKCS12 file in a secret, then decoded and imported into the keychain during the workflow.

## Common failures and fixes

I have hit every failure mode listed below at least once.

**Notarization fails with "The binary is not signed."** You forgot `--deep` in the codesign command, or a nested framework was not signed. Run `codesign --verify --deep --strict MyApp.app` to find the unsigned component.

**Windows SmartScreen still blocks the app after signing.** New certificates lack reputation. You need to build up download volume, or buy an EV certificate that has immediate reputation.

**AppImage will not launch on Ubuntu.** You are probably missing the FUSE library. Either install `libfuse2` or extract the AppImage with `--appimage-extract` and run the resulting `AppRun` binary.

**DMG opens with the wrong window size.** The `.DS_Store` file in the DMG controls window position and icon layout. Use `create-dmg` or set it manually with AppleScript before sealing the DMG.

## Summary

Distributing a Rust desktop app requires platform-specific knowledge that has nothing to do with Rust itself. The build tooling (cargo), the language, and the framework are the same across platforms. The distribution process is three completely different workflows bolted together with CI scripts.

The investment in codesigning and notarization pays off quickly. Users trust signed apps. Auto-updates keep them on the latest version. Ed25519 manifest signing protects against server compromise. For any production desktop application, these are requirements, not optional extras.

gpui-starter handles codesigning, DMG/MSI/AppImage packaging, and Ed25519-signed auto-updates out of the box. The [getting started guide](/docs/docs/getting-started/) walks through the full build and release setup. The source code is on [GitHub](https://github.com/freeoxide/gpui-starter).

For more on the broader Rust desktop ecosystem, including how GPUI compares to other frameworks, see the [framework comparison post](/blog/electron-vs-tauri-vs-gpui/).
