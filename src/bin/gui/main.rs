#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;

use anyhow::{Context, Result};
use fs4::{FileExt, TryLockError};
use std::fs::{self, OpenOptions};

fn main() -> Result<()> {
    let app_dir = anime2ctxdb::app_data_dir()?;
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

    let series_root = app_dir;

    eframe::run_native(
        "anime2ctxdb",
        eframe::NativeOptions {
            renderer: eframe::Renderer::Wgpu,
            viewport: eframe::egui::ViewportBuilder::default()
                .with_inner_size([1100.0, 760.0])
                .with_min_inner_size([800.0, 500.0]),
            ..Default::default()
        },
        Box::new(|creation| {
            app::set_fonts(&creation.egui_ctx);
            app::set_style(&creation.egui_ctx);
            Ok(Box::new(app::App::new(series_root)))
        }),
    )?;
    Ok(())
}
