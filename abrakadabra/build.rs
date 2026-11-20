//! Embed `data/app.ico` and a manifest into each executable.
//!
//! MinGW's `windres` turns the .ico and the manifest into a COFF object with a
//! proper `.rsrc` section, so Explorer, the task bar and the settings window all
//! show the key-cap icon (the white key with the black Z and the red Я).
//!
//! Two objects are built because the two binaries need different privileges:
//!   * `z_single_app`     -> zя.exe, `asInvoker` (the keyboard hook does not
//!     need elevation; elevation comes from the Task Scheduler task)
//!   * `z_single_launcher` -> z.exe, `requireAdministrator` (the installer
//!     writes into %APPDATA% and registers an elevated autostart task)
//! If `windres` is not on PATH the build continues without these resources.

fn main() {
    let out_dir = std::env::var("OUT_DIR").unwrap_or_else(|_| ".".to_string());
    let ico = std::path::Path::new("data/app.ico");
    if !ico.exists() {
        println!("cargo:warning=data/app.ico is missing, the exe gets no icon");
        return;
    }

    let abs_icon = ico
        .canonicalize()
        .unwrap_or_else(|_| ico.to_path_buf())
        .to_string_lossy()
        .replace("\\\\?\\", "")
        .replace('\\', "/");

    let windres = std::env::var("PATH")
        .unwrap_or_default()
        .split(';')
        .map(|path| std::path::PathBuf::from(path).join("windres.exe"))
        .find(|path| path.exists());

    let windres = match windres {
        Some(found) => found,
        None => {
            println!("cargo:warning=windres not found on PATH, no application icon embedded");
            return;
        }
    };

    let targets = [
        ("z_single_app", "asInvoker", "zya_app.rc", "zya_app.obj"),
        (
            "z_single_launcher",
            "requireAdministrator",
            "zya_launcher.rc",
            "zya_launcher.obj",
        ),
    ];

    for (bin, level, rc_name, obj_name) in targets {
        let rc = std::path::PathBuf::from(&out_dir).join(rc_name);
        let obj = std::path::PathBuf::from(&out_dir).join(obj_name);
        let manifest = std::path::PathBuf::from(&out_dir).join(format!("{bin}.manifest.xml"));

        std::fs::write(
            &manifest,
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
                 <assembly xmlns=\"urn:schemas-microsoft-com:asm.v1\" manifestVersion=\"1.0\">\n\
                 <trustInfo xmlns=\"urn:schemas-microsoft-com:asm.v2\">\n\
                 <security>\n\
                 <requestedPrivileges xmlns=\"urn:schemas-microsoft-com:asm.v3\">\n\
                 <requestedExecutionLevel level=\"{level}\" uiAccess=\"false\"/>\n\
                 </requestedPrivileges>\n\
                 </security>\n\
                 </trustInfo>\n\
                 </assembly>\n"
            ),
        )
        .unwrap_or_else(|e| println!("cargo:warning=cannot write manifest for {bin}: {e}"));

        let manifest_path = manifest.to_string_lossy().replace('\\', "/");

        std::fs::write(
            &rc,
            format!("1 ICON \"{abs_icon}\"\n1 10 \"{manifest_path}\"\n"),
        )
        .unwrap_or_else(|e| println!("cargo:warning=cannot write {rc_name}: {e}"));

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
                println!("cargo:rustc-link-arg-bin={bin}={}", obj.display());
            }
            Ok(out) => println!(
                "cargo:warning=windres failed for {bin}: {}",
                String::from_utf8_lossy(&out.stderr)
            ),
            Err(e) => println!("cargo:warning=cannot run windres: {e}"),
        }
    }
}
