#![cfg_attr(all(windows, not(test)), windows_subsystem = "windows")]

mod app;
mod deps;
mod downloader;
mod probe;
mod ytdlp;

use std::sync::Arc;

use eframe::egui;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("YouTube MP4 下载器")
            .with_inner_size([820.0, 860.0])
            .with_min_inner_size([660.0, 560.0]),
        ..Default::default()
    };

    eframe::run_native(
        "YouTube MP4 下载器",
        options,
        Box::new(|cc| {
            install_cjk_font(&cc.egui_ctx);
            app::apply_theme(&cc.egui_ctx);
            Ok(Box::new(app::DownloaderApp::new()))
        }),
    )
}

fn install_cjk_font(ctx: &egui::Context) {
    let candidates: &[(&str, u32)] = &[
        (r"C:\Windows\Fonts\msyh.ttc", 0),
        (r"C:\Windows\Fonts\Deng.ttf", 0),
        (r"C:\Windows\Fonts\simhei.ttf", 0),
        (r"C:\Windows\Fonts\simsun.ttc", 0),
        (r"/System/Library/Fonts/PingFang.ttc", 0),
        ("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc", 2),
        ("/usr/share/fonts/truetype/wqy/wqy-microhei.ttc", 0),
    ];

    for (path, index) in candidates {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };

        let mut data = egui::FontData::from_owned(bytes);
        data.index = *index;

        let mut fonts = egui::FontDefinitions::default();
        fonts.font_data.insert("cjk".to_owned(), Arc::new(data));
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .insert(0, "cjk".to_owned());
        fonts
            .families
            .entry(egui::FontFamily::Monospace)
            .or_default()
            .push("cjk".to_owned());
        ctx.set_fonts(fonts);
        return;
    }
}
