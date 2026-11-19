//! Embed `data/app.ico` and `data/manifest.xml` into the executable.
//!
//! MinGW's `windres` turns the .ico and manifest into a COFF object that carries
//! a proper `.rsrc` section. The icon is used by Explorer and the task bar, while
//! the manifest declares the launch level and makes the app DPI aware.
//! If `windres` is not on PATH the build continues without these resources.

fn main() {
    let out_dir = std::env::var("OUT_DIR").unwrap_or_else(|_| ".".to_string());
    let ico = std::path::Path::new("data/app.ico");
    if !ico.exists() {
        println!("cargo:warning=data/app.ico is missing, the exe gets no icon");
        return;
    }

    let manifest = std::path::Path::new("data/manifest.xml");
    if !manifest.exists() {
        std::fs::write(
            manifest,
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<assembly xmlns=\"urn:schemas-microsoft-com:asm.v1\" manifestVersion=\"1.0\">\n  <trustInfo xmlns=\"urn:schemas-microsoft-com:asm.v2\">\n    <security>\n      <requestedPrivileges xmlns=\"urn:schemas-microsoft-com:asm.v3\">\n        <requestedExecutionLevel level=\"asInvoker\" uiAccess=\"false\"/>\n      </requestedPrivileges>\n    </security>\n  </trustInfo>\n</assembly>\n",
        )
        .unwrap_or_else(|e| println!("cargo:warning=cannot create data/manifest.xml: {e}"));
    }

    let abs_icon = ico
        .canonicalize()
        .unwrap_or_else(|_| ico.to_path_buf())
        .to_string_lossy()
        .replace("\\\\?\\", "")
        .replace('\\', "/");
    let abs_manifest = manifest
        .canonicalize()
        .unwrap_or_else(|_| manifest.to_path_buf())
        .to_string_lossy()
        .replace("\\\\?\\", "")
        .replace('\\', "/");

    let rc = std::path::PathBuf::from(&out_dir).join("zya.rc");
    let obj = std::path::PathBuf::from(&out_dir).join("zya_icon.obj");

    std::fs::write(
        &rc,
        format!("1 ICON \"{abs_icon}\"\n1 10 \"{abs_manifest}\"\n"),
    )
    .unwrap_or_else(|e| println!("cargo:warning=cannot write resource rc: {e}"));

    let windres = std::env::var("PATH")
        .unwrap_or_default()
        .split(';')
        .map(|p| std::path::PathBuf::from(p).join("windres.exe"))
        .find(|p| p.exists());

    let windres = match windres {
        Some(w) => w,
        None => {
            println!("cargo:warning=windres not found on PATH, no application icon embedded");
            return;
        }
    };

    let output = std::process::Command::new(&windres)
        .arg("-i")
        .arg(&rc)
        .arg("-O")
        .arg("coff")
        .arg("-o")
        .arg(&obj)
        .output();

    match output {
        Ok(out) if out.status.success() => {
            println!("cargo:rustc-link-arg={}", obj.display());
        }
        Ok(out) => {
            println!(
                "cargo:warning=windres failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
        Err(e) => println!("cargo:warning=cannot run windres: {e}"),
    }
}
