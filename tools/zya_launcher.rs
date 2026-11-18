// Wrapper for the release binary. It makes the public name `zya.exe` behave
// like the application: with no parameters it starts the normal working mode.
// The original Rust app is embedded and extracted to the temporary directory.

const APP: &[u8] = include_bytes!("../release/zya_app.exe");

fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let app_path = std::env::temp_dir().join("zya_app.exe");

    if std::fs::metadata(&app_path).map(|m| m.len() != APP.len() as u64).unwrap_or(true) {
        std::fs::write(&app_path, APP)?;
    }

    let mut cmd = std::process::Command::new(&app_path);
    if args.is_empty() {
        cmd.args(["run", "headless"]);
    } else {
        cmd.args(args);
    }

    let status = cmd.status()?;
    std::process::exit(status.code().unwrap_or(1));
}
