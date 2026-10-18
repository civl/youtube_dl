use std::{
    collections::VecDeque,
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::mpsc::{self, Receiver},
    time::Duration,
};

use eframe::egui;
use egui::{Align, Align2, Color32, CornerRadius, FontId, Layout, Margin, RichText, Stroke, Vec2};

use crate::{
    deps::{self, InstallEvent, Tools},
    downloader::{self, AuthMode, Codec, DownloadEvent, DownloadHandle},
    probe::{self, ProbeEvent},
};

pub const BG: Color32 = Color32::from_rgb(14, 16, 22);
pub const CARD: Color32 = Color32::from_rgb(23, 26, 35);
pub const CARD_HOVER: Color32 = Color32::from_rgb(30, 34, 45);
pub const INPUT: Color32 = Color32::from_rgb(17, 20, 27);
pub const BORDER: Color32 = Color32::from_rgb(40, 45, 58);
pub const BORDER_HI: Color32 = Color32::from_rgb(58, 65, 84);
pub const TEXT: Color32 = Color32::from_rgb(232, 235, 242);
pub const MUTED: Color32 = Color32::from_rgb(148, 156, 174);
pub const FAINT: Color32 = Color32::from_rgb(104, 112, 132);
pub const ACCENT: Color32 = Color32::from_rgb(233, 69, 96);
pub const ACCENT_SOFT: Color32 = Color32::from_rgb(45, 28, 36);
pub const SUCCESS: Color32 = Color32::from_rgb(61, 214, 140);
pub const WARNING: Color32 = Color32::from_rgb(245, 166, 35);
pub const ERROR: Color32 = Color32::from_rgb(255, 107, 122);

pub fn apply_theme(ctx: &egui::Context) {
    ctx.set_theme(egui::ThemePreference::Dark);

    ctx.all_styles_mut(|style| {
        let visuals = &mut style.visuals;

        *visuals = egui::Visuals::dark();
        visuals.panel_fill = BG;
        visuals.window_fill = BG;
        visuals.extreme_bg_color = INPUT;
        visuals.faint_bg_color = CARD;
        visuals.override_text_color = Some(TEXT);
        visuals.hyperlink_color = ACCENT;
        visuals.text_edit_bg_color = Some(INPUT);
        visuals.selection.bg_fill = ACCENT.gamma_multiply(0.45);
        visuals.selection.stroke = Stroke::new(1.0, Color32::WHITE);

        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
        visuals.widgets.inactive.weak_bg_fill = CARD;
        visuals.widgets.inactive.bg_fill = CARD;
        visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER);
        visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
        visuals.widgets.inactive.corner_radius = CornerRadius::same(8);
        visuals.widgets.hovered.weak_bg_fill = CARD_HOVER;
        visuals.widgets.hovered.bg_fill = CARD_HOVER;
        visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, BORDER_HI);
        visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, Color32::WHITE);
        visuals.widgets.hovered.corner_radius = CornerRadius::same(8);
        visuals.widgets.active.weak_bg_fill = ACCENT_SOFT;
        visuals.widgets.active.bg_fill = ACCENT_SOFT;
        visuals.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);
        visuals.widgets.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);
        visuals.widgets.active.corner_radius = CornerRadius::same(8);

        style.spacing.item_spacing = Vec2::new(10.0, 8.0);
        style.spacing.button_padding = Vec2::new(14.0, 8.0);
        style.text_styles = [
            (
                egui::TextStyle::Heading,
                FontId::new(24.0, egui::FontFamily::Proportional),
            ),
            (
                egui::TextStyle::Body,
                FontId::new(14.0, egui::FontFamily::Proportional),
            ),
            (
                egui::TextStyle::Button,
                FontId::new(14.0, egui::FontFamily::Proportional),
            ),
            (
                egui::TextStyle::Small,
                FontId::new(12.0, egui::FontFamily::Proportional),
            ),
            (
                egui::TextStyle::Monospace,
                FontId::new(11.5, egui::FontFamily::Monospace),
            ),
        ]
        .into();
    });
}

pub struct DownloaderApp {
    url: String,
    codec: Codec,
    resolution: Option<u32>,
    qualities: probe::Heights,
    probe: ProbeState,
    probing_url: Option<String>,
    cookies_enabled: bool,
    cookies_path: String,
    output_dir: String,
    tools: Tools,
    install: InstallState,
    job: Option<Job>,
    progress: f32,
    stage: String,
    logs: VecDeque<String>,
    last_output: Option<PathBuf>,
    error: Option<String>,
    cancelled: bool,
    compat_retry_used: bool,
    deps_error: Option<String>,
}

struct Job {
    handle: DownloadHandle,
}

#[derive(Default)]
struct InstallState {
    running: bool,
    status: String,
    progress: f32,
    rx: Option<Receiver<InstallEvent>>,
}

#[derive(Default)]
struct ProbeState {
    running: bool,
    error: Option<String>,
    rx: Option<Receiver<ProbeEvent>>,
}

impl DownloaderApp {
    pub fn new() -> Self {
        let output_dir = dirs::download_dir()
            .or_else(|| dirs::home_dir().map(|home| home.join("Downloads")))
            .unwrap_or_else(|| PathBuf::from("."));
        let bundled_error = deps::ensure_bundled_tools()
            .err()
            .map(|error| format!("内置依赖准备失败：{error}"));

        Self {
            url: String::new(),
            codec: Codec::H264,
            resolution: None,
            qualities: probe::Heights::default(),
            probe: ProbeState::default(),
            probing_url: None,
            cookies_enabled: false,
            cookies_path: String::new(),
            output_dir: output_dir.display().to_string(),
            tools: deps::detect_tools(),
            install: InstallState::default(),
            job: None,
            progress: 0.0,
            stage: String::new(),
            logs: VecDeque::new(),
            last_output: None,
            error: None,
            cancelled: false,
            compat_retry_used: false,
            deps_error: bundled_error,
        }
    }

    fn poll_background(&mut self, ctx: &egui::Context) {
        let mut events = Vec::new();
        if let Some(job) = &self.job {
            while let Ok(event) = job.handle.events.try_recv() {
                events.push(event);
            }
        }
        for event in events {
            match event {
                DownloadEvent::Log(line) => self.push_log(line),
                DownloadEvent::Progress { progress, stage } => {
                    self.progress = progress;
                    self.stage = stage;
                }
                DownloadEvent::Stage(stage) => self.stage = stage,
                DownloadEvent::Done(path) => {
                    self.progress = 1.0;
                    self.stage = "下载完成".into();
                    self.push_log(format!("已保存：{}", path.display()));
                    self.last_output = Some(path);
                    self.cancelled = false;
                    self.error = None;
                    self.job = None;
                }
                DownloadEvent::Cancelled => {
                    self.stage = "已取消下载".into();
                    self.cancelled = true;
                    self.push_log("下载已取消");
                    self.job = None;
                }
                DownloadEvent::Failed(message) => {
                    self.stage = "下载失败".into();
                    self.push_log(format!("错误：{message}"));
                    self.job = None;
                    let can_retry =
                        self.cookies_enabled && !self.compat_retry_used && message.contains("403");
                    if can_retry {
                        self.compat_retry_used = true;
                        self.error = None;
                        self.push_log("当前直链被 YouTube 拒绝（HTTP 403），正在切换兼容模式重试…");
                        self.start_download(ctx, true);
                    } else {
                        self.error = Some(message);
                    }
                }
            }
        }

        let mut install_events = Vec::new();
        if let Some(rx) = &self.install.rx {
            while let Ok(event) = rx.try_recv() {
                install_events.push(event);
            }
        }
        for event in install_events {
            match event {
                InstallEvent::Status(status) => {
                    self.install.status = status;
                    self.install.progress = 0.0;
                }
                InstallEvent::Progress {
                    label,
                    downloaded,
                    total,
                } => {
                    let fraction = total
                        .filter(|total| *total > 0)
                        .map(|total| downloaded as f32 / total as f32);
                    self.install.progress = fraction.unwrap_or(0.0);
                    self.install.status = match total {
                        Some(total) => format!(
                            "正在下载 {label}… {} / {}",
                            human_bytes(downloaded),
                            human_bytes(total)
                        ),
                        None => format!("正在下载 {label}… {}", human_bytes(downloaded)),
                    };
                }
                InstallEvent::Finished(result) => {
                    self.install.running = false;
                    self.install.rx = None;
                    match result {
                        Ok(tools) => {
                            self.install.progress = 1.0;
                            self.install.status = "依赖已就绪".into();
                            self.tools = tools;
                            self.deps_error = None;
                            self.push_log("依赖安装完成");
                        }
                        Err(error) => {
                            self.push_log(format!("依赖安装失败：{error}"));
                            self.deps_error = Some(error);
                        }
                    }
                }
            }
        }

        let mut probe_events = Vec::new();
        if let Some(rx) = &self.probe.rx {
            while let Ok(event) = rx.try_recv() {
                probe_events.push(event);
            }
        }
        for event in probe_events {
            match event {
                ProbeEvent::Finished(result) => {
                    self.probe.running = false;
                    self.probe.rx = None;
                    let matches_current_url = self.probing_url.as_deref() == Some(self.url.trim());
                    self.probing_url = None;

                    match result {
                        Ok(info) if matches_current_url => {
                            self.qualities = info.heights;
                            if self.resolution.is_some_and(|height| {
                                !self.qualities.for_codec(self.codec).contains(&height)
                            }) {
                                self.resolution = None;
                            }
                            self.probe.error = None;
                            let heights = self
                                .qualities
                                .all
                                .iter()
                                .map(|height| format!("{height}p"))
                                .collect::<Vec<_>>()
                                .join(" / ");
                            self.push_log(format!("解析完成：《{}》", info.title));
                            self.push_log(format!("可选清晰度：{heights}"));
                        }
                        Ok(_) => {}
                        Err(error) if matches_current_url => {
                            self.push_log(error.clone());
                            self.probe.error = Some(error);
                        }
                        Err(_) => {}
                    }
                }
            }
        }

        if self.job.is_some() || self.install.running || self.probe.running {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }

    fn push_log(&mut self, line: impl Into<String>) {
        let line = line.into();
        if line.trim().is_empty() {
            return;
        }
        self.logs.push_back(line);
        while self.logs.len() > 300 {
            self.logs.pop_front();
        }
    }

    fn pick_cookies_file(&mut self) {
        let mut dialog = rfd::FileDialog::new()
            .add_filter("cookies.txt", &["txt"])
            .add_filter("所有文件", &["*"]);
        let current = PathBuf::from(self.cookies_path.trim());
        if let Some(parent) = current.parent().filter(|parent| parent.is_dir()) {
            dialog = dialog.set_directory(parent);
        }
        if let Some(file) = dialog.pick_file() {
            self.cookies_path = file.display().to_string();
        }
    }

    fn pick_folder(&mut self) {
        let mut dialog = rfd::FileDialog::new();
        let current = PathBuf::from(self.output_dir.trim());
        if current.is_dir() {
            dialog = dialog.set_directory(&current);
        } else if let Some(downloads) = dirs::download_dir() {
            dialog = dialog.set_directory(downloads);
        }
        if let Some(folder) = dialog.pick_folder() {
            self.output_dir = folder.display().to_string();
        }
    }

    fn install_dependencies(&mut self) {
        if self.install.running || self.job.is_some() || self.probe.running {
            return;
        }
        let upgrading = self.tools.ready();
        let (tx, rx) = mpsc::channel();
        self.deps_error = None;
        self.install = InstallState {
            running: true,
            status: if upgrading {
                "准备升级依赖…".into()
            } else {
                "准备安装缺失依赖…".into()
            },
            progress: 0.0,
            rx: Some(rx),
        };
        if upgrading {
            self.push_log("开始升级依赖（下载最新版本）");
        } else {
            self.push_log("开始安装缺失依赖");
        }
        deps::start_install(self.tools.clone(), upgrading, tx);
    }

    fn start_probe(&mut self) {
        if self.probe.running {
            return;
        }
        self.probe.error = None;

        let url = self.url.trim().to_string();
        if url.is_empty() {
            self.probe.error = Some("请先粘贴 YouTube 视频链接。".into());
            return;
        }
        if !looks_like_youtube_url(&url) {
            self.probe.error =
                Some("链接格式不正确，请提供 youtube.com 或 youtu.be 的视频地址。".into());
            return;
        }
        let cookies = match resolve_cookies(self.cookies_enabled, &self.cookies_path) {
            Ok(cookies) => cookies,
            Err(error) => {
                self.probe.error = Some(error);
                return;
            }
        };
        if cookies.is_some() && self.tools.js_runtime.is_none() {
            self.probe.error =
                Some("低隐私模式需要 QuickJS 运行时，请点击底部「升级依赖」补齐后再试。".into());
            return;
        }
        let Some(yt_dlp) = &self.tools.yt_dlp else {
            self.probe.error = Some("缺少 yt-dlp，请先安装依赖。".into());
            return;
        };
        let js_runtime = cookies.as_ref().and(self.tools.js_runtime.as_deref());

        let handle = probe::start(yt_dlp, &url, cookies.as_deref(), js_runtime);
        self.probe.running = true;
        self.probe.rx = Some(handle.events);
        self.probing_url = Some(url.clone());
        self.push_log(format!("正在解析清晰度：{url}"));
        if cookies.is_some() {
            self.push_log("低隐私模式：cookies.txt + QuickJS，使用登录态客户端解析");
        }
    }

    fn start_download(&mut self, ctx: &egui::Context, compatible: bool) {
        if !compatible {
            self.compat_retry_used = false;
        }
        self.error = None;
        self.cancelled = false;
        self.last_output = None;

        let url = self.url.trim().to_string();
        if url.is_empty() {
            self.error = Some("请先粘贴 YouTube 视频链接。".into());
            return;
        }
        if !looks_like_youtube_url(&url) {
            self.error = Some("链接格式不正确，请提供 youtube.com 或 youtu.be 的视频地址。".into());
            return;
        }
        let cookies = match resolve_cookies(self.cookies_enabled, &self.cookies_path) {
            Ok(cookies) => cookies,
            Err(error) => {
                self.error = Some(error);
                return;
            }
        };

        let folder_text = self.output_dir.trim().to_string();
        if folder_text.is_empty() {
            self.error = Some("请选择保存文件夹。".into());
            return;
        }
        let output_dir = PathBuf::from(&folder_text);
        if let Err(error) = fs::create_dir_all(&output_dir) {
            self.error = Some(format!("无法创建保存文件夹：{error}"));
            return;
        }

        self.tools = deps::detect_tools();
        if !self.tools.ready() {
            self.error = Some("缺少 yt-dlp 或 ffmpeg，请先在页面底部安装依赖。".into());
            return;
        }
        if cookies.is_some() && self.tools.js_runtime.is_none() {
            self.error =
                Some("低隐私模式需要 QuickJS 运行时，请点击底部「升级依赖」补齐后再试。".into());
            return;
        }
        let js_runtime = cookies.as_ref().and(self.tools.js_runtime.as_deref());
        let auth_mode = if compatible {
            AuthMode::Compatible
        } else {
            AuthMode::Default
        };

        let resolution = self.resolution;
        let has_cookies = cookies.is_some();
        match downloader::start(
            &self.tools,
            &url,
            self.codec,
            resolution,
            &output_dir,
            downloader::Auth {
                cookies: cookies.as_deref(),
                js_runtime,
                mode: auth_mode,
            },
        ) {
            Ok(handle) => {
                if !compatible {
                    self.logs.clear();
                }
                self.progress = 0.0;
                self.stage = "正在解析视频信息…".into();
                self.push_log(format!("开始下载：{url}"));
                self.push_log(format!(
                    "编码 {} · 清晰度 {} · 保存至 {}",
                    self.codec.name(),
                    if compatible {
                        "兼容模式最高可用".to_string()
                    } else {
                        resolution
                            .map(format_height)
                            .unwrap_or_else(|| "最高可用".into())
                    },
                    output_dir.display()
                ));
                if has_cookies {
                    if compatible {
                        self.push_log("低隐私模式：兼容模式（web_embedded）重试，清晰度可能降低");
                    } else {
                        self.push_log("低隐私模式：cookies.txt + QuickJS，使用登录态客户端解析");
                    }
                }
                self.job = Some(Job { handle });
                ctx.request_repaint();
            }
            Err(error) => {
                self.error = Some(error);
            }
        }
    }

    fn render(&mut self, ui: &mut egui::Ui) {
        self.header(ui);
        ui.add_space(20.0);
        self.url_section(ui);
        ui.add_space(16.0);
        self.cookies_section(ui);
        ui.add_space(16.0);
        self.codec_section(ui);
        ui.add_space(16.0);
        self.resolution_section(ui);
        ui.add_space(16.0);
        self.folder_section(ui);
        ui.add_space(20.0);
        self.action_section(ui);

        if self.job.is_some()
            || self.last_output.is_some()
            || self.error.is_some()
            || self.cancelled
        {
            ui.add_space(16.0);
            self.status_section(ui);
        }

        ui.add_space(14.0);
        self.log_section(ui);
        ui.add_space(6.0);
        self.deps_section(ui);
        ui.add_space(6.0);
    }

    fn header(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(Vec2::new(46.0, 46.0), egui::Sense::hover());
            ui.painter()
                .rect_filled(rect, CornerRadius::same(12), ACCENT);
            ui.painter().text(
                rect.center(),
                Align2::CENTER_CENTER,
                "MP4",
                FontId::proportional(13.0),
                Color32::WHITE,
            );
            ui.add_space(2.0);
            ui.vertical(|ui| {
                ui.label(RichText::new("YouTube MP4 下载器").size(24.0).strong());
                ui.label(
                    RichText::new("粘贴链接，选择编码，自动下载并合并为 MP4")
                        .size(12.5)
                        .color(MUTED),
                );
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new("Rust · egui").size(11.0).color(FAINT));
            });
        });
    }

    fn url_section(&mut self, ui: &mut egui::Ui) {
        field_label(ui, "视频链接", "支持 youtube.com / youtu.be");
        let response = ui.add_sized(
            [ui.available_width(), 38.0],
            egui::TextEdit::singleline(&mut self.url)
                .hint_text("https://www.youtube.com/watch?v=…")
                .font(egui::TextStyle::Body)
                .margin(Margin::symmetric(12, 9))
                .background_color(INPUT),
        );

        if response.changed() {
            self.qualities = probe::Heights::default();
            self.resolution = None;
            self.probe.error = None;
        }
    }

    fn cookies_section(&mut self, ui: &mut egui::Ui) {
        field_label(ui, "登录内容", "需要登录 / 年龄验证的视频");
        let editable = self.job.is_none() && !self.probe.running && !self.install.running;
        let mut browse_clicked = false;

        ui.add_enabled_ui(editable, |ui| {
            ui.checkbox(
                &mut self.cookies_enabled,
                RichText::new("低隐私模式：使用 cookies.txt 下载登录可见内容").size(13.0),
            );
        });

        if self.cookies_enabled {
            ui.add_space(6.0);
            ui.add_enabled_ui(editable, |ui| {
                ui.horizontal(|ui| {
                    let spacing = ui.spacing().item_spacing.x;
                    let button_width = 112.0;
                    let edit_width = (ui.available_width() - button_width - spacing).max(140.0);
                    ui.add_sized(
                        [edit_width, 34.0],
                        egui::TextEdit::singleline(&mut self.cookies_path)
                            .hint_text("选择浏览器导出的 cookies.txt")
                            .font(egui::TextStyle::Body)
                            .margin(Margin::symmetric(12, 8))
                            .background_color(INPUT),
                    );
                    let browse = egui::Button::new(RichText::new("选择文件…").size(13.0))
                        .min_size(Vec2::new(button_width, 34.0))
                        .corner_radius(8);
                    if ui.add(browse).clicked() {
                        browse_clicked = true;
                    }
                });
            });
            ui.add_space(5.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                ui.label(
                    RichText::new("仅将文件路径传给本机 yt-dlp，不上传、不保存副本。导出工具：")
                        .size(11.0)
                        .color(FAINT),
                );
                ui.hyperlink_to(
                    RichText::new("Get cookies.txt LOCALLY").size(11.0),
                    "https://chromewebstore.google.com/detail/get-cookiestxt-locally/cclelndahbckbenkjhflpdbgdldlbecc",
                );
            });
            ui.label(
                RichText::new("cookies 等同于登录凭证，请勿分享 cookies.txt。")
                    .size(11.0)
                    .color(WARNING),
            );
            ui.label(
                RichText::new("若高清直链被 YouTube 拒绝，会自动以兼容模式重试，清晰度可能降低。")
                    .size(11.0)
                    .color(FAINT),
            );
            if self.tools.js_runtime.is_none() {
                ui.label(
                    RichText::new(
                        "缺少 QuickJS 运行时，登录内容暂时无法解析，请点击底部「升级依赖」。",
                    )
                    .size(11.0)
                    .color(WARNING),
                );
            }
        } else {
            ui.label(
                RichText::new("默认关闭，只下载公开可访问的内容")
                    .size(11.0)
                    .color(FAINT),
            );
        }

        if browse_clicked {
            self.pick_cookies_file();
        }
    }

    fn codec_section(&mut self, ui: &mut egui::Ui) {
        field_label(ui, "MP4 编码", "选择期望的视频编码");
        let enabled = self.job.is_none();
        let mut selected = self.codec;

        ui.columns(3, |columns| {
            for (index, codec) in Codec::ALL.into_iter().enumerate() {
                if codec_card(&mut columns[index], codec, codec == selected, enabled) {
                    selected = codec;
                }
            }
        });
        self.codec = selected;
        if self
            .resolution
            .is_some_and(|height| !self.qualities.for_codec(self.codec).contains(&height))
        {
            self.resolution = None;
        }

        ui.add_space(5.0);
        ui.label(
            RichText::new("实际编码取决于源视频；若源不提供所选编码，将自动回退到最佳可用流。")
                .size(11.0)
                .color(FAINT),
        );
    }

    fn resolution_section(&mut self, ui: &mut egui::Ui) {
        field_label(ui, "清晰度", "解析后可选，默认为最高清晰度");

        let busy = self.job.is_some() || self.install.running || self.probe.running;
        let heights = self.qualities.for_codec(self.codec).to_vec();
        let selected_label = match self.resolution {
            Some(height) => format_height(height),
            None => match heights.first() {
                Some(height) => format!("最高可用 · {}", format_height(*height)),
                None => "最高可用".to_string(),
            },
        };
        let mut probe_clicked = false;

        ui.horizontal(|ui| {
            ui.add_enabled_ui(!heights.is_empty() && !busy, |ui| {
                egui::ComboBox::from_id_salt("resolution_combo")
                    .selected_text(RichText::new(selected_label).size(13.0))
                    .width(200.0)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.resolution, None, "最高可用");
                        for height in &heights {
                            ui.selectable_value(
                                &mut self.resolution,
                                Some(*height),
                                format_height(*height),
                            );
                        }
                    });
            });

            let button_text = if self.probe.running {
                "解析中…"
            } else if heights.is_empty() {
                "解析清晰度"
            } else {
                "重新解析"
            };
            let button = egui::Button::new(RichText::new(button_text).size(13.0))
                .fill(CARD)
                .stroke(Stroke::new(1.0, BORDER))
                .corner_radius(8)
                .min_size(Vec2::new(112.0, 36.0));
            if ui.add_enabled(!busy, button).clicked() {
                probe_clicked = true;
            }

            if self.probe.running {
                ui.add(egui::Spinner::new().size(14.0).color(ACCENT));
                ui.label(RichText::new("正在解析视频信息…").size(11.5).color(MUTED));
            } else if !heights.is_empty() {
                let summary = heights
                    .iter()
                    .map(|height| format!("{height}p"))
                    .collect::<Vec<_>>()
                    .join(" / ");
                ui.label(RichText::new(summary).size(11.0).color(FAINT));
            } else if !self.qualities.all.is_empty() {
                ui.label(
                    RichText::new("当前编码无可用清晰度，将自动回退")
                        .size(11.0)
                        .color(WARNING),
                );
            } else {
                ui.label(
                    RichText::new("未解析时自动选择最高清晰度")
                        .size(11.0)
                        .color(FAINT),
                );
            }
        });

        if let Some(error) = &self.probe.error {
            ui.add_space(4.0);
            ui.label(RichText::new(error).size(11.0).color(ERROR));
        }

        if probe_clicked {
            self.start_probe();
        }
    }

    fn folder_section(&mut self, ui: &mut egui::Ui) {
        field_label(ui, "保存位置", "默认为系统下载文件夹");
        let mut browse_clicked = false;

        ui.horizontal(|ui| {
            let spacing = ui.spacing().item_spacing.x;
            let button_width = 112.0;
            let edit_width = (ui.available_width() - button_width - spacing).max(140.0);
            ui.add_sized(
                [edit_width, 36.0],
                egui::TextEdit::singleline(&mut self.output_dir)
                    .hint_text("选择或输入文件夹路径")
                    .font(egui::TextStyle::Body)
                    .margin(Margin::symmetric(12, 8))
                    .background_color(INPUT),
            );
            let browse = egui::Button::new(RichText::new("选择文件夹…").size(13.0))
                .min_size(Vec2::new(button_width, 36.0))
                .corner_radius(8);
            if ui
                .add_enabled(self.job.is_none(), browse)
                .on_hover_text("打开系统文件夹选择器")
                .clicked()
            {
                browse_clicked = true;
            }
        });

        if browse_clicked {
            self.pick_folder();
        }
    }

    fn action_section(&mut self, ui: &mut egui::Ui) {
        let running = self.job.is_some();
        let can_start = !running && !self.install.running && !self.probe.running;
        let mut start_clicked = false;
        let mut cancel_clicked = false;
        let mut open_clicked = false;

        ui.horizontal(|ui| {
            let label = if running {
                "正在下载…"
            } else {
                "开始下载"
            };
            let start_button = egui::Button::new(
                RichText::new(label)
                    .size(14.5)
                    .strong()
                    .color(Color32::WHITE),
            )
            .fill(if running {
                Color32::from_rgb(81, 40, 50)
            } else {
                ACCENT
            })
            .stroke(Stroke::NONE)
            .corner_radius(10)
            .min_size(Vec2::new(150.0, 42.0));

            if ui.add_enabled(can_start, start_button).clicked() {
                start_clicked = true;
            }

            if running {
                let cancel_button = egui::Button::new(RichText::new("取消").size(13.5))
                    .fill(CARD)
                    .stroke(Stroke::new(1.0, BORDER))
                    .corner_radius(10)
                    .min_size(Vec2::new(92.0, 42.0));
                if ui.add(cancel_button).clicked() {
                    cancel_clicked = true;
                }
            }

            if self.last_output.is_some() {
                let open_button =
                    egui::Button::new(RichText::new("打开文件夹").size(13.5).color(TEXT))
                        .fill(CARD)
                        .stroke(Stroke::new(1.0, BORDER))
                        .corner_radius(10)
                        .min_size(Vec2::new(120.0, 42.0));
                if ui.add(open_button).clicked() {
                    open_clicked = true;
                }
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new("输出格式  MP4").size(11.0).color(FAINT));
            });
        });

        if start_clicked {
            self.start_download(ui.ctx(), false);
        }
        if cancel_clicked && let Some(job) = &self.job {
            job.handle.cancel();
            self.stage = "正在取消…".into();
            self.push_log("正在取消下载…");
        }
        if open_clicked && let Some(path) = &self.last_output {
            open_folder(path);
        }
    }

    fn status_section(&mut self, ui: &mut egui::Ui) {
        let running = self.job.is_some();
        let (fill, border) = if self.error.is_some() {
            (Color32::from_rgb(37, 24, 29), ERROR.gamma_multiply(0.7))
        } else if self.last_output.is_some() {
            (Color32::from_rgb(20, 34, 29), SUCCESS.gamma_multiply(0.5))
        } else {
            (CARD, BORDER)
        };

        egui::Frame::NONE
            .fill(fill)
            .stroke(Stroke::new(1.0, border))
            .corner_radius(12)
            .inner_margin(Margin::same(16))
            .show(ui, |ui| {
                if let Some(error) = &self.error {
                    ui.label(RichText::new("下载失败").size(13.5).strong().color(ERROR));
                    ui.add_space(4.0);
                    ui.label(RichText::new(error).size(12.5).color(TEXT));
                } else if let Some(path) = &self.last_output {
                    ui.label(RichText::new("下载完成").size(13.5).strong().color(SUCCESS));
                    ui.add_space(4.0);
                    let file_name = path
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_else(|| path.display().to_string());
                    ui.label(RichText::new(file_name).size(13.0).strong());
                    ui.label(
                        RichText::new(path.display().to_string())
                            .size(11.0)
                            .color(MUTED),
                    );
                } else if self.cancelled {
                    ui.label(RichText::new("已取消下载").size(13.5).strong().color(MUTED));
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new("未完成的临时文件可能保留在保存文件夹中。")
                            .size(11.5)
                            .color(FAINT),
                    );
                } else if running {
                    ui.horizontal(|ui| {
                        ui.add(egui::Spinner::new().size(15.0).color(ACCENT));
                        ui.label(RichText::new(&self.stage).size(13.0));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(
                                RichText::new(format!("{:.0}%", self.progress * 100.0))
                                    .size(12.0)
                                    .color(MUTED),
                            );
                        });
                    });
                    ui.add_space(8.0);
                    ui.add(
                        egui::ProgressBar::new(self.progress)
                            .desired_height(8.0)
                            .fill(ACCENT)
                            .corner_radius(4),
                    );
                }
            });
    }

    fn log_section(&mut self, ui: &mut egui::Ui) {
        if self.logs.is_empty() {
            return;
        }

        egui::CollapsingHeader::new(RichText::new("运行日志").size(12.0).color(MUTED))
            .id_salt("run_log")
            .default_open(false)
            .show(ui, |ui| {
                egui::Frame::NONE
                    .fill(INPUT)
                    .corner_radius(8)
                    .inner_margin(Margin::same(10))
                    .show(ui, |ui| {
                        egui::ScrollArea::vertical()
                            .max_height(150.0)
                            .stick_to_bottom(true)
                            .auto_shrink([false, true])
                            .show(ui, |ui| {
                                for line in &self.logs {
                                    ui.label(
                                        RichText::new(line).monospace().size(11.0).color(FAINT),
                                    );
                                }
                            });
                    });
            });
    }

    fn deps_section(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        ui.add_space(6.0);

        let mut install_clicked = false;
        let mut refresh_clicked = false;

        ui.horizontal(|ui| {
            ui.label(RichText::new("依赖").size(11.0).color(FAINT));
            dependency_chip(ui, "yt-dlp", &self.tools.yt_dlp);
            dependency_chip(ui, "ffmpeg", &self.tools.ffmpeg);
            dependency_chip(ui, "QuickJS", &self.tools.js_runtime);

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if self.install.running {
                    ui.add(egui::Spinner::new().size(13.0).color(WARNING));
                    ui.label(RichText::new(&self.install.status).size(11.0).color(MUTED));
                } else {
                    if ui
                        .add_enabled(
                            self.job.is_none() && !self.probe.running,
                            egui::Button::new(RichText::new("重新检测").size(11.5))
                                .fill(Color32::TRANSPARENT)
                                .stroke(Stroke::NONE),
                        )
                        .clicked()
                    {
                        refresh_clicked = true;
                    }

                    let ready = self.tools.ready();
                    let install = if ready {
                        egui::Button::new(RichText::new("升级依赖").size(11.5).color(MUTED))
                            .fill(Color32::TRANSPARENT)
                            .stroke(Stroke::new(1.0, BORDER))
                            .corner_radius(7)
                            .min_size(Vec2::new(84.0, 28.0))
                    } else {
                        egui::Button::new(
                            RichText::new("安装缺失依赖")
                                .size(12.0)
                                .strong()
                                .color(Color32::WHITE),
                        )
                        .fill(ACCENT)
                        .stroke(Stroke::NONE)
                        .corner_radius(7)
                        .min_size(Vec2::new(112.0, 28.0))
                    };
                    let hint = if ready {
                        "在线下载最新版 yt-dlp、ffmpeg 与 QuickJS 并替换内置版本（约 100 MB）"
                    } else {
                        "从 GitHub / gyan.dev 下载 yt-dlp、ffmpeg 与 QuickJS（约 100 MB）"
                    };
                    if ui
                        .add_enabled(self.job.is_none() && !self.probe.running, install)
                        .on_hover_text(hint)
                        .clicked()
                    {
                        install_clicked = true;
                    }
                }
            });
        });

        if self.install.running && self.install.progress > 0.0 {
            ui.add_space(4.0);
            ui.add(
                egui::ProgressBar::new(self.install.progress)
                    .desired_height(5.0)
                    .fill(ACCENT)
                    .animate(self.install.progress < 1.0),
            );
        }

        if let Some(error) = &self.deps_error {
            ui.add_space(4.0);
            ui.label(RichText::new(error).size(11.0).color(ERROR));
        }

        if install_clicked {
            self.install_dependencies();
        }
        if refresh_clicked {
            self.tools = deps::detect_tools();
            self.deps_error = None;
            if self.tools.ready() {
                self.push_log("依赖已就绪");
            } else {
                self.push_log("仍未检测到 yt-dlp 或 ffmpeg");
            }
        }
    }
}

impl eframe::App for DownloaderApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll_background(ui.ctx());

        egui::Frame::central_panel(ui.style())
            .inner_margin(Margin::symmetric(24, 18))
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| self.render(ui));
            });
    }
}

fn field_label(ui: &mut egui::Ui, title: &str, hint: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(title).size(13.0).strong().color(TEXT));
        ui.label(RichText::new(hint).size(11.0).color(FAINT));
    });
    ui.add_space(5.0);
}

fn codec_card(ui: &mut egui::Ui, codec: Codec, selected: bool, enabled: bool) -> bool {
    let fill = if selected { ACCENT_SOFT } else { CARD };
    let border = if selected { ACCENT } else { BORDER };

    let response = egui::Frame::NONE
        .fill(fill)
        .stroke(Stroke::new(if selected { 1.5 } else { 1.0 }, border))
        .corner_radius(11)
        .inner_margin(Margin::same(14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_min_height(80.0);
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(codec.name())
                            .size(17.0)
                            .strong()
                            .color(if selected { ACCENT } else { TEXT }),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let (rect, _) =
                            ui.allocate_exact_size(Vec2::new(18.0, 18.0), egui::Sense::hover());
                        let center = rect.center();
                        ui.painter().circle_stroke(
                            center,
                            7.5,
                            Stroke::new(1.2, if selected { ACCENT } else { BORDER_HI }),
                        );
                        if selected {
                            ui.painter().circle_filled(center, 4.2, ACCENT);
                        }
                    });
                });
                ui.add_space(3.0);
                ui.label(
                    RichText::new(codec.tagline())
                        .size(12.5)
                        .strong()
                        .color(if selected { TEXT } else { MUTED }),
                );
                ui.add_space(2.0);
                ui.label(RichText::new(codec.detail()).size(11.0).color(FAINT));
            });
        })
        .response;

    let response = response.interact(if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    });
    let response = if enabled {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    };

    enabled && response.clicked()
}

fn dependency_chip(ui: &mut egui::Ui, name: &str, path: &Option<PathBuf>) {
    let (color, status) = if path.is_some() {
        (SUCCESS, "已就绪")
    } else {
        (WARNING, "未安装")
    };

    let response = ui
        .horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(Vec2::new(8.0, 8.0), egui::Sense::hover());
            ui.painter().circle_filled(rect.center(), 3.5, color);
            ui.label(
                RichText::new(format!("{name} · {status}"))
                    .size(11.0)
                    .color(if path.is_some() { MUTED } else { WARNING }),
            );
        })
        .response;

    if let Some(path) = path {
        response.on_hover_text(path.display().to_string());
    }
}

fn looks_like_youtube_url(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    let Some((scheme, rest)) = lower.split_once("://") else {
        return false;
    };
    if scheme != "http" && scheme != "https" {
        return false;
    }

    let authority = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .rsplit('@')
        .next()
        .unwrap_or_default();
    let host = authority.split(':').next().unwrap_or_default();

    host == "youtube.com"
        || host.ends_with(".youtube.com")
        || host == "youtu.be"
        || host == "youtube-nocookie.com"
        || host.ends_with(".youtube-nocookie.com")
}

fn resolve_cookies(enabled: bool, raw: &str) -> Result<Option<PathBuf>, String> {
    if !enabled {
        return Ok(None);
    }

    let raw = raw.trim();
    if raw.is_empty() {
        return Err("低隐私模式已开启，请先选择 cookies.txt 文件。".into());
    }
    let path = PathBuf::from(raw);
    if !path.is_file() {
        return Err(format!("cookies.txt 不存在或不是文件：{}", path.display()));
    }
    Ok(Some(path))
}

fn format_height(height: u32) -> String {
    match height {
        4320 => "4320p (8K)".into(),
        2160 => "2160p (4K)".into(),
        1440 => "1440p (2K)".into(),
        1080 => "1080p (Full HD)".into(),
        720 => "720p (HD)".into(),
        _ => format!("{height}p"),
    }
}

fn human_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;
    let bytes = bytes as f64;
    if bytes >= GIB {
        format!("{:.1} GB", bytes / GIB)
    } else if bytes >= MIB {
        format!("{:.1} MB", bytes / MIB)
    } else if bytes >= KIB {
        format!("{:.0} KB", bytes / KIB)
    } else {
        format!("{bytes:.0} B")
    }
}

fn open_folder(file: &Path) {
    let folder = file.parent().unwrap_or(file);

    #[cfg(target_os = "windows")]
    {
        let _ = Command::new("explorer").arg(folder).spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = Command::new("open").arg(folder).spawn();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let _ = Command::new("xdg-open").arg(folder).spawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_common_youtube_links() {
        assert!(looks_like_youtube_url(
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ"
        ));
        assert!(looks_like_youtube_url("https://youtu.be/dQw4w9WgXcQ"));
        assert!(looks_like_youtube_url(
            "https://music.youtube.com/watch?v=dQw4w9WgXcQ"
        ));
        assert!(looks_like_youtube_url(
            "http://m.youtube.com/watch?v=dQw4w9WgXcQ"
        ));
    }

    #[test]
    fn rejects_lookalike_hosts() {
        assert!(!looks_like_youtube_url("https://notyoutube.com/watch?v=x"));
        assert!(!looks_like_youtube_url(
            "https://youtube.com.example.com/watch?v=x"
        ));
        assert!(!looks_like_youtube_url("ftp://youtube.com/watch?v=x"));
        assert!(!looks_like_youtube_url("youtube.com/watch?v=x"));
    }

    #[test]
    fn formats_human_readable_sizes() {
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(1024), "1 KB");
        assert_eq!(human_bytes(2 * 1024 * 1024), "2.0 MB");
    }

    #[test]
    fn formats_common_heights() {
        assert_eq!(format_height(2160), "2160p (4K)");
        assert_eq!(format_height(1080), "1080p (Full HD)");
        assert_eq!(format_height(480), "480p");
    }

    #[test]
    fn resolves_cookies_only_when_enabled() {
        assert!(resolve_cookies(false, "").unwrap().is_none());
        assert!(resolve_cookies(true, "   ").is_err());
        assert!(resolve_cookies(true, "definitely-missing-cookies.txt").is_err());

        let path = std::env::temp_dir().join(format!(
            "ymp4-test-cookies-{}-{}.txt",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|elapsed| elapsed.as_nanos())
                .unwrap_or_default()
        ));
        fs::write(&path, "# Netscape HTTP Cookie File\n").unwrap();
        let resolved = resolve_cookies(true, &path.display().to_string()).unwrap();
        assert_eq!(resolved.as_deref(), Some(path.as_path()));
        let _ = fs::remove_file(path);
    }
}
