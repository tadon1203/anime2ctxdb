#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;

use anyhow::{Context, Result, bail};
use fs4::{FileExt, TryLockError};
use std::fs::{self, OpenOptions};

fn main() -> Result<()> {
    let Some(data_dir) = dirs::data_local_dir() else {
        bail!("no local data directory on this system");
    };
    let app_dir = data_dir.join("anime2ctxdb");
    fs::create_dir_all(&app_dir).with_context(|| format!("cannot create {}", app_dir.display()))?;

    // The lock lives until the process ends. A second instance cannot get it.
    let lock_path = app_dir.join("app.lock");
    let lock_file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&lock_path)
        .with_context(|| format!("cannot open {}", lock_path.display()))?;
    match FileExt::try_lock(&lock_file) {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => {
            // The dialog result does not change the exit code, so it is dropped on purpose.
            let _ = rfd::MessageDialog::new()
                .set_level(rfd::MessageLevel::Error)
                .set_title("anime2ctxdb")
                .set_description("anime2ctxdb is already running")
                .show();
            std::process::exit(1);
        }
        Err(TryLockError::Error(error)) => {
            return Err(error).with_context(|| format!("cannot lock {}", lock_path.display()));
        }
    }

    eframe::run_native(
        "anime2ctxdb",
        eframe::NativeOptions {
            renderer: eframe::Renderer::Wgpu,
            ..Default::default()
        },
        Box::new(|creation| {
            app::set_sizes(&creation.egui_ctx);
            Ok(Box::new(app::App::default()))
        }),
    )?;
    drop(lock_file);
    Ok(())
}
