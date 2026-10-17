//! Bounded native smoke test. Synthetic files only; no listener, services, prefs or recovery scanning.
//! Run: mbx run -p figcraft --example sync_smoke -- /absolute/new/evidence-directory
use eframe::egui;
use figcraft_engine::Session;
use figcraft_ui_egui::{FigApp, NoServices};
use serde_json::json;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

struct Smoke {
    app: FigApp,
    dir: PathBuf,
    started: Instant,
    stage: u8,
    frame: u64,
    requested: bool,
    error: Option<String>,
    hidden_write_frame: Option<u64>,
    hidden_verified: bool,
    capture_after: Instant,
    verified_screenshots: u8,
}

fn execute(s: &mut Session, cmd: &str, p: serde_json::Value) -> Result<serde_json::Value, String> {
    s.execute(cmd, &p).map_err(|e| e.to_string())
}
fn create(name: &str, color: &str, path: &PathBuf) -> Result<Session, String> {
    let mut s = Session::new();
    execute(&mut s, "file.rename", json!({"name": name}))?;
    execute(&mut s, "shape.rectangle", json!({"name": "Smoke rectangle", "x": 80, "y": 80, "width": 520, "height": 320, "props": {"fill": color}}))?;
    execute(&mut s, "text.create", json!({"name": "Smoke text", "x": 110, "y": 125, "text": name, "props": {"fontSize": 24, "fill": "#FFFFFF"}}))?;
    execute(&mut s, "file.save", json!({"path": path}))?;
    Ok(s)
}
impl Smoke {
    fn new(dir: PathBuf) -> Result<Self, String> {
        std::fs::create_dir(&dir).map_err(|e| e.to_string())?;
        let session = create("SYNTHETIC — original", "#375AD9", &dir.join("original.figcraft"))?;
        let mut app = FigApp::new(session, Box::new(NoServices));
        app.ui.home = false;
        Ok(Self {
            app,
            dir,
            started: Instant::now(),
            stage: 0,
            frame: 0,
            requested: false,
            error: None,
            hidden_write_frame: None,
            hidden_verified: false,
            capture_after: Instant::now(),
            verified_screenshots: 0,
        })
    }
    fn change_external(&self, name: &str, color: &str) -> Result<(), String> {
        let mut other = Session::empty();
        execute(&mut other, "file.open", json!({"path": self.dir.join("original.figcraft")}))?;
        execute(&mut other, "file.rename", json!({"name": name}))?;
        let text = other.doc().map_err(|e| e.to_string())?.nodes().find(|n| n.name == "Smoke text").map(|n| n.id).ok_or("synthetic text absent")?;
        execute(&mut other, "node.set", json!({"ids": [text], "props": {"characters": name}}))?;
        let rect = other
            .doc()
            .map_err(|e| e.to_string())?
            .nodes()
            .find(|n| n.name == "Smoke rectangle")
            .map(|n| n.id)
            .ok_or("synthetic rectangle absent")?;
        execute(&mut other, "node.set", json!({"ids": [rect], "props": {"fill": color}}))?;
        execute(&mut other, "file.save", json!({}))?;
        Ok(())
    }
    /// The explicit refresh an agent calls after an external write; the app does not poll.
    fn refresh(&mut self) -> Result<serde_json::Value, String> {
        let target = figcraft_engine::target::of(self.app.session.ds().ok());
        execute(&mut self.app.session, "file.refresh", json!({"target": target}))
    }
    fn tick(&mut self, ctx: &egui::Context) -> Result<(), String> {
        let elapsed = self.started.elapsed();
        if elapsed > Duration::from_secs(30) {
            return Err(format!(
                "native smoke timeout at stage {}, frames {}, screenshot requested {}, viewport {:?}",
                self.stage,
                self.frame,
                self.requested,
                ctx.input(|i| i.viewport().clone())
            ));
        }
        let previous = self.stage;
        match self.stage {
            0 if self.frame > 2 => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                self.stage = 1;
            }
            1 if elapsed > Duration::from_secs(2) && ctx.input(|i| i.viewport().minimized == Some(true)) => {
                self.change_external("SYNTHETIC — external refreshed", "#7F46B8")?;
                self.hidden_write_frame = Some(self.frame);
                let refreshed = self.refresh()?;
                if refreshed["state"] != "refreshed" || self.app.session.doc().map_err(|e| e.to_string())?.name != "SYNTHETIC — external refreshed"
                {
                    return Err(format!("explicit refresh while minimized failed: {refreshed}"));
                }
                self.hidden_verified = ctx.input(|i| i.viewport().minimized == Some(true)) && self.hidden_write_frame == Some(self.frame);
                if !self.hidden_verified {
                    return Err("refresh did not occur while minimized without UI frames".into());
                }
                let status = execute(&mut self.app.session, "file.status", json!({}))?;
                if status["savedAndCurrent"] != true {
                    return Err("refreshed live/disk state differs".into());
                }
                std::fs::write(self.dir.join("refresh-status.json"), status.to_string()).map_err(|e| e.to_string())?;
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                self.stage = 3;
                self.capture_after = Instant::now() + Duration::from_secs(2);
            }
            4 => {
                self.change_external("SYNTHETIC — visible refresh", "#147C57")?;
                let refreshed = self.refresh()?;
                if refreshed["state"] != "refreshed" {
                    return Err(format!("explicit refresh while visible failed: {refreshed}"));
                }
                std::fs::write(self.dir.join("visible-refresh.json"), refreshed.to_string()).map_err(|e| e.to_string())?;
                self.stage = 5;
                self.capture_after = Instant::now() + Duration::from_secs(2);
            }
            6 => {
                execute(&mut self.app.session, "file.rename", json!({"name": "SYNTHETIC — LOCAL UNSAVED"}))?;
                self.change_external("SYNTHETIC — external conflict", "#7F46B8")?;
                let refreshed = self.refresh()?;
                if refreshed["state"] != "conflict" {
                    return Err(format!("local edits did not produce a conflict: {refreshed}"));
                }
                self.stage = 7;
            }
            7 => {
                if self.app.session.doc().map_err(|e| e.to_string())?.name != "SYNTHETIC — LOCAL UNSAVED" {
                    return Err("local work lost".into());
                }
                let path = self.dir.join("original.figcraft");
                let before = std::fs::read(&path).map_err(|e| e.to_string())?;
                if execute(&mut self.app.session, "file.save", json!({})).is_ok() {
                    return Err("conflicting save unexpectedly succeeded".into());
                }
                if before != std::fs::read(&path).map_err(|e| e.to_string())? {
                    return Err("refused save changed external bytes".into());
                }
                let status = execute(&mut self.app.session, "file.status", json!({}))?;
                std::fs::write(self.dir.join("conflict-status.json"), status.to_string()).map_err(|e| e.to_string())?;
                self.stage = 8;
                self.capture_after = Instant::now() + Duration::from_secs(2);
            }
            9 => {
                std::fs::write(
                    self.dir.join("result.json"),
                    json!({"ok": true, "hiddenRefresh": self.hidden_verified, "noFramesDuringHiddenRefresh": self.hidden_verified, "visibleRefresh": true, "conflictPreserved": true, "verifiedScreenshots": self.verified_screenshots, "frames": self.frame}).to_string(),
                )
                .map_err(|e| e.to_string())?;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            _ => {}
        }
        if self.stage != previous {
            eprintln!("sync_smoke: stage {} at {:?}, frames {}", self.stage, elapsed, self.frame);
        }
        ctx.request_repaint_after(Duration::from_millis(100));
        Ok(())
    }
    fn fail(&mut self, ctx: &egui::Context, error: String) {
        eprintln!("sync_smoke: {error}");
        self.error = Some(error.clone());
        let _ = std::fs::write(self.dir.join("result.json"), json!({"ok": false, "error": error, "stage": self.stage}).to_string());
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
}
impl eframe::App for Smoke {
    fn logic(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        if self.error.is_none()
            && let Err(e) = self.tick(ctx)
        {
            self.fail(ctx, e);
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        self.frame += 1;
        let ctx = ui.ctx().clone();
        self.app.ui(ui);
        let shots = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Screenshot { image, .. } => Some(image.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>()
        });
        for image in shots {
            let name = match self.stage {
                3 => "01-refreshed.png",
                5 => "02-visible-refresh.png",
                8 => "03-conflict.png",
                _ => continue,
            };
            if image.size[0] < 1200 || image.size[1] < 800 {
                self.fail(&ctx, format!("{name}: viewport too small for visual evidence: {:?}", image.size));
                return;
            }
            let rgba = image.pixels.iter().flat_map(|p| p.to_srgba_unmultiplied()).collect::<Vec<_>>();
            match image::RgbaImage::from_raw(image.size[0] as u32, image.size[1] as u32, rgba)
                .ok_or("bad screenshot".to_string())
                .and_then(|im| im.save(self.dir.join(name)).map_err(|e| e.to_string()))
            {
                Ok(()) => {
                    // The canvas renders asynchronously. A correct document status alone can still
                    // accompany an old canvas texture. Check the actual captured pixels as well.
                    let expected = if self.stage == 3 { [0x7f, 0x46, 0xb8] } else { [0x14, 0x7c, 0x57] };
                    let matching = image
                        .pixels
                        .iter()
                        .filter(|p| {
                            let [r, g, b, a] = p.to_srgba_unmultiplied();
                            a == 255 && [r, g, b].into_iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= 3)
                        })
                        .count();
                    if matching < image.pixels.len() / 40 || matching < 1000 {
                        self.fail(&ctx, format!("{name}: expected canvas color absent ({matching} pixels)"));
                        return;
                    }
                    self.verified_screenshots += 1;
                    self.stage += 1;
                    self.requested = false;
                }
                Err(e) => self.fail(&ctx, e),
            }
        }
        if matches!(self.stage, 3 | 5 | 8) && !self.requested && Instant::now() >= self.capture_after {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
            self.requested = true;
        }
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = std::env::args().nth(1).map(PathBuf::from).ok_or("pass a new absolute synthetic evidence directory")?;
    if !dir.is_absolute() || dir.exists() {
        return Err("evidence directory must be absolute and must not exist".into());
    }
    let smoke = Smoke::new(dir.clone()).map_err(std::io::Error::other)?;
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("FigCraft synthetic synchronization smoke")
            .with_inner_size([1200.0, 800.0])
            .with_min_inner_size([1200.0, 800.0])
            .with_resizable(false),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    eframe::run_native("FigCraft synthetic synchronization smoke", options, Box::new(move |_| Ok(Box::new(smoke))))?;
    let result: serde_json::Value = serde_json::from_slice(&std::fs::read(dir.join("result.json"))?)?;
    if result["ok"] != true || result["verifiedScreenshots"] != 3 {
        return Err(format!("native smoke failed: {result}").into());
    }
    Ok(())
}
