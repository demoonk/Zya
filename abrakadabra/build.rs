//! Embed `data/app.ico` into the executable as its application icon.
//!
//! MinGW's `windres` turns the .ico into a COFF object that carries a proper
//! `.rsrc` section; linking that object into the binary is enough for Explorer
//! and the task bar to show the icon. If `windres` is not on PATH the build
//! simply continues without the icon.

fn main() {
    let out_dir = std::env::var("OUT_DIR").unwrap_or_else(|_| ".".to_string());
    let ico = std::path::Path::new("data/app.ico");
    if !ico.exists() {
        println!("cargo:warning=data/app.ico is missing, the exe gets no icon");
        return;
    }

    let abs = ico
        .canonicalize()
        .unwrap_or_else(|_| ico.to_path_buf())
        .to_string_lossy()
        .replace("\\\\?\\", "")
        .replace('\\', "/");
    let rc = std::path::PathBuf::from(&out_dir).join("zya.rc");
    let obj = std::path::PathBuf::from(&out_dir).join("zya_icon.obj");

    std::fs::write(&rc, format!("1 ICON \"{abs}\"\n"))
        .unwrap_or_else(|e| println!("cargo:warning=cannot write icon rc: {e}"));

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
