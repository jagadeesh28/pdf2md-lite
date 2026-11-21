use std::path::Path;
use std::process::Command;
use std::time::SystemTime;

/// Checks if any UI source files or configs have been modified after the compiled dist index.
fn should_rebuild_ui(ui_dir: &Path, dist_index: &Path) -> bool {
    if !dist_index.exists() {
        return true;
    }

    let Ok(dist_meta) = dist_index.metadata() else {
        return true;
    };
    let Ok(dist_mtime) = dist_meta.modified() else {
        return true;
    };

    fn is_newer(path: &Path, dist_mtime: SystemTime) -> bool {
        if !path.exists() {
            return false;
        }
        if path.is_file() {
            if let Ok(meta) = path.metadata()
                && let Ok(mtime) = meta.modified()
            {
                return mtime > dist_mtime;
            }
            return false;
        }
        if path.is_dir()
            && let Ok(entries) = std::fs::read_dir(path)
        {
            for entry in entries.flatten() {
                let entry_path = entry.path();
                let name = entry.file_name();
                if name == "node_modules" || name == "dist" || name == ".git" {
                    continue;
                }
                if entry_path.is_dir() {
                    if is_newer(&entry_path, dist_mtime) {
                        return true;
                    }
                } else if let Ok(meta) = entry.metadata()
                    && let Ok(mtime) = meta.modified()
                    && mtime > dist_mtime
                {
                    return true;
                }
            }
        }
        false
    }

    let check_targets = [
        ui_dir.join("src"),
        ui_dir.join("public"),
        ui_dir.join("package.json"),
        ui_dir.join("vite.config.ts"),
        ui_dir.join("index.html"),
        ui_dir.join("tsconfig.json"),
        ui_dir.join("tsconfig.app.json"),
        ui_dir.join("tsconfig.node.json"),
    ];

    check_targets.iter().any(|p| is_newer(p, dist_mtime))
}

fn main() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let ui_dir = manifest_dir.join("../../ui");
    let dist_index = ui_dir.join("dist").join("index.html");

    // Re-run build.rs if any frontend source files or configurations change
    println!("cargo:rerun-if-changed=../../ui/src");
    println!("cargo:rerun-if-changed=../../ui/public");
    println!("cargo:rerun-if-changed=../../ui/package.json");
    println!("cargo:rerun-if-changed=../../ui/vite.config.ts");
    println!("cargo:rerun-if-changed=../../ui/index.html");
    println!("cargo:rerun-if-changed=../../ui/tsconfig.json");
    println!("cargo:rerun-if-changed=../../ui/tsconfig.app.json");
    println!("cargo:rerun-if-changed=../../ui/tsconfig.node.json");
    println!("cargo:rerun-if-changed=build.rs");

    // Determine npm binary depending on platform
    let npm_cmd = if cfg!(windows) { "npm.cmd" } else { "npm" };

    // 1. Install dependencies if node_modules is missing
    if !ui_dir.join("node_modules").exists() {
        println!("cargo:warning=[pdfmd-lite-desktop] Installing npm dependencies...");
        match Command::new(npm_cmd).current_dir(&ui_dir).arg("install").status() {
            Ok(status) if status.success() => {
                println!("cargo:warning=[pdfmd-lite-desktop] Successfully installed npm dependencies.");
            }
            Ok(status) => {
                panic!("'npm install' failed with exit status: {status}");
            }
            Err(err) => {
                if dist_index.exists() {
                    println!("cargo:warning=[pdfmd-lite-desktop] Warning: Failed to run 'npm install' ({err}), but pre-built dist/ exists. Continuing.");
                } else {
                    panic!("Failed to execute 'npm install': {err}. Please ensure Node.js and npm are installed.");
                }
            }
        }
    }

    // 2. Build frontend assets if missing or newer than dist/index.html
    if should_rebuild_ui(&ui_dir, &dist_index) {
        println!("cargo:warning=[pdfmd-lite-desktop] Building frontend bundle (npm run build)...");
        match Command::new(npm_cmd)
            .current_dir(&ui_dir)
            .args(["run", "build"])
            .status()
        {
            Ok(status) if status.success() => {
                println!("cargo:warning=[pdfmd-lite-desktop] Frontend build completed successfully.");
            }
            Ok(status) => {
                panic!("'npm run build' failed with exit status: {status}");
            }
            Err(err) => {
                if dist_index.exists() {
                    println!("cargo:warning=[pdfmd-lite-desktop] Warning: Failed to execute 'npm run build' ({err}), but pre-built dist/ exists. Continuing.");
                } else {
                    panic!("Failed to execute 'npm run build': {err}. Please ensure Node.js and npm are installed.");
                }
            }
        }
    } else {
        println!("cargo:warning=[pdfmd-lite-desktop] Frontend bundle (ui/dist) is up to date. Skipping npm run build.");
    }

    // 3. Delegate to Tauri build script to embed dist/ and icons
    tauri_build::build();
}
