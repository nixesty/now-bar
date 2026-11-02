// The setup's window (ui/setup.slint), its manifest and icon (the app's), and its payload: with
// PEBBLY_SETUP_PAYLOAD set (a folder holding release pebbly.exe and pebbly-settings.exe) both are
// packed into OUT_DIR/payload.bin; without it the payload is empty (the window works, "Kur" says
// the setup carries no files). The exe embeds it with include_bytes!: nothing is appended to the
// file, so signing it later stays possible.
use std::path::{Path, PathBuf};
use std::process::Command;

#[path = "src/payload.rs"]
#[allow(dead_code)]
mod payload;

fn main() {
    // At run time, as in the app's build.rs: a compiled build script is shared between checkouts.
    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    // The app's English, bundled under this crate's name (Slint looks for <crate>.po).
    let po = root.join("..").join("i18n").join("en").join("LC_MESSAGES").join("pebbly-ui.po");
    println!("cargo:rerun-if-changed={}", po.display());
    let bundle = out.join("i18n").join("en").join("LC_MESSAGES");
    std::fs::create_dir_all(&bundle).expect("i18n bundle dir");
    std::fs::copy(&po, bundle.join("pebbly-setup.po")).expect("pebbly-ui.po");
    let config = slint_build::CompilerConfiguration::new()
        .with_bundled_translations(out.join("i18n"))
        .with_default_translation_context(slint_build::DefaultTranslationContext::None);
    slint_build::compile_with_config("ui/setup.slint", config).expect("setup/ui/setup.slint");

    println!("cargo:rerun-if-env-changed=PEBBLY_SETUP_PAYLOAD");
    println!("cargo:rerun-if-env-changed=PEBBLY_VERSION");
    let packed = match std::env::var_os("PEBBLY_SETUP_PAYLOAD") {
        Some(dir) => {
            let dir = PathBuf::from(dir);
            let names = ["pebbly.exe", "pebbly-settings.exe"];
            let files: Vec<(&str, Vec<u8>)> = names
                .iter()
                .map(|n| {
                    let path = dir.join(n);
                    println!("cargo:rerun-if-changed={}", path.display());
                    (*n, std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display())))
                })
                .collect();
            let refs: Vec<(&str, &[u8])> = files.iter().map(|(n, b)| (*n, b.as_slice())).collect();
            let version = std::env::var("PEBBLY_VERSION").unwrap_or_default();
            payload::pack(&version, &refs)
        }
        None => Vec::new(),
    };
    std::fs::write(out.join("payload.bin"), packed).expect("payload.bin");

    let manifest = out.join("pebbly.manifest");
    let app = root.parent().expect("the repo");
    println!("cargo:rerun-if-changed={}", app.join("pebbly.manifest").display());
    std::fs::copy(app.join("pebbly.manifest"), &manifest).expect("pebbly.manifest");
    println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
    println!("cargo:rustc-link-arg-bins=/MANIFESTINPUT:{}", manifest.display());
    embed_icon(&app.join("assets").join("pebbly.ico"), &out);
}

/// As the app's build.rs: rc.exe from the Windows SDK, or the default icon and a warning.
fn embed_icon(source: &Path, out: &Path) {
    println!("cargo:rerun-if-changed={}", source.display());
    let Some(rc) = find_rc() else {
        println!("cargo:warning=rc.exe (Windows SDK) not found: the setup keeps the default icon");
        return;
    };
    let ico = out.join("pebbly.ico");
    std::fs::copy(source, &ico).expect("pebbly.ico");
    let script = out.join("setup.rc");
    let path = ico.display().to_string().replace('\\', "\\\\");
    std::fs::write(&script, format!("1 ICON \"{path}\"\n")).expect("setup.rc");
    let res = out.join("setup.res");
    let built = Command::new(rc).arg("/nologo").arg("/fo").arg(&res).arg(&script).status().is_ok_and(|s| s.success());
    if built {
        println!("cargo:rustc-link-arg-bins={}", res.display());
    } else {
        println!("cargo:warning=rc.exe failed: the setup keeps the default icon");
    }
}

fn find_rc() -> Option<PathBuf> {
    let bin = Path::new(r"C:\Program Files (x86)\Windows Kits\10\bin");
    let mut found: Vec<PathBuf> = std::fs::read_dir(bin).ok()?.filter_map(|e| e.ok()).map(|e| e.path().join("x64").join("rc.exe")).filter(|p| p.exists()).collect();
    found.sort();
    found.pop()
}
