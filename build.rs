// Embeds a Windows version resource into the exe, so Explorer shows the version
// under Properties > Details. The numbers come from `version` in Cargo.toml.
//
// Works for native Windows builds (rc.exe from the Windows SDK) and for
// cross-builds from Linux with cargo-xwin (llvm-rc).

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=Cargo.toml");

    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let version = env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION is set by cargo");
    let number = |name: &str| -> u16 {
        env::var(name)
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0)
    };
    let (major, minor, patch) = (
        number("CARGO_PKG_VERSION_MAJOR"),
        number("CARGO_PKG_VERSION_MINOR"),
        number("CARGO_PKG_VERSION_PATCH"),
    );
    let description = env::var("CARGO_PKG_DESCRIPTION").unwrap_or_default();
    let name = env::var("CARGO_PKG_NAME").unwrap_or_default();

    // Exe icon shown by Explorer, generated from code with
    // `cargo run --release --features assets -- --render-icon assets/nemotron-bubble.ico`.
    // The first icon group in the exe is the one Explorer uses.
    let icon = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set by cargo"))
        .join("assets")
        .join("nemotron-bubble.ico");
    println!("cargo:rerun-if-changed={}", icon.display());
    // Forward slashes: valid on Windows and avoids escape sequences in the .rc string.
    let icon_path = icon.display().to_string().replace('\\', "/");

    // Plain numeric constants instead of #include <winver.h>, so no SDK headers
    // are needed to compile the resource.
    let rc = format!(
        r#"1 ICON "{icon_path}"

1 VERSIONINFO
FILEVERSION {major},{minor},{patch},0
PRODUCTVERSION {major},{minor},{patch},0
FILEFLAGSMASK 0x3F
FILEFLAGS 0x0
FILEOS 0x40004
FILETYPE 0x1
FILESUBTYPE 0x0
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "FileDescription", "{description}"
      VALUE "FileVersion", "{version}"
      VALUE "InternalName", "{name}"
      VALUE "OriginalFilename", "{name}.exe"
      VALUE "ProductName", "Nemotron Bubble"
      VALUE "ProductVersion", "{version}"
      VALUE "LegalCopyright", "MIT License"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#
    );

    let out = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR is set by cargo")).join("version.rc");
    fs::write(&out, rc).expect("write version.rc");
    embed_resource::compile(&out, embed_resource::NONE)
        .manifest_optional()
        .expect("compile the version resource");
}
