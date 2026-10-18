use std::{
    fs,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread,
    time::{Duration, SystemTime},
};

use crate::deps::Tools;

const OUTPUT_MARKER: &str = "__YMP4_OUTPUT__";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthMode {
    Default,
    Compatible,
}

pub struct Auth<'a> {
    pub cookies: Option<&'a Path>,
    pub js_runtime: Option<&'a Path>,
    pub mode: AuthMode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Codec {
    H264,
    H265,
    Av1,
}

impl Codec {
    pub const ALL: [Codec; 3] = [Codec::H264, Codec::H265, Codec::Av1];

    pub fn name(self) -> &'static str {
        match self {
            Codec::H264 => "H.264",
            Codec::H265 => "H.265",
            Codec::Av1 => "AV1",
        }
    }

    pub fn tagline(self) -> &'static str {
        match self {
            Codec::H264 => "兼容性最佳",
            Codec::H265 => "更小体积",
            Codec::Av1 => "最高压缩率",
        }
    }

    pub fn detail(self) -> &'static str {
        match self {
            Codec::H264 => "几乎所有播放器与设备都能直接播放，推荐日常使用。",
            Codec::H265 => "同等画质下文件更小，需要设备支持 HEVC 解码。",
            Codec::Av1 => "新一代编码，体积最小，适合较新的设备与软件。",
        }
    }

    fn format_selector(self, height: Option<u32>) -> String {
        let Some(height) = height else {
            return match self {
                Codec::H264 => {
                    "bv*[vcodec^=avc1][ext=mp4]+ba[ext=m4a]/b[ext=mp4][vcodec^=avc1]/b[ext=mp4]/b"
                }
                Codec::H265 => {
                    "bv*[vcodec^=hev1]+ba[ext=m4a]/bv*[vcodec^=hvc1]+ba[ext=m4a]/bv*[vcodec^=hev1]+ba/bv*[vcodec^=hvc1]+ba/bv*[vcodec^=hev1]/bv*[vcodec^=hvc1]/b"
                }
                Codec::Av1 => "bv*[vcodec^=av01]+ba[ext=m4a]/bv*[vcodec^=av01]+ba/b",
            }
            .to_string();
        };

        match self {
            Codec::H264 => format!(
                "bv*[height={height}][vcodec^=avc1][ext=mp4]+ba[ext=m4a]/bv*[height={height}][vcodec^=avc1]+ba[ext=m4a]/bv*[height={height}][vcodec^=avc1]+ba/b[height={height}][ext=mp4][vcodec^=avc1]/b[height={height}][ext=mp4]/bv*[height={height}]+ba/b[height={height}]"
            ),
            Codec::H265 => format!(
                "bv*[height={height}][vcodec^=hev1]+ba[ext=m4a]/bv*[height={height}][vcodec^=hvc1]+ba[ext=m4a]/bv*[height={height}][vcodec^=hev1]+ba/bv*[height={height}][vcodec^=hvc1]+ba/bv*[height={height}]+ba/b[height={height}]"
            ),
            Codec::Av1 => format!(
                "bv*[height={height}][vcodec^=av01]+ba[ext=m4a]/bv*[height={height}][vcodec^=av01]+ba/bv*[height={height}]+ba/b[height={height}]"
            ),
        }
    }
}

#[derive(Debug)]
pub enum DownloadEvent {
    Log(String),
    Progress { progress: f32, stage: String },
    Stage(String),
    Done(PathBuf),
    Cancelled,
    Failed(String),
}

pub struct DownloadHandle {
    pub events: Receiver<DownloadEvent>,
    child: Arc<Mutex<Option<Child>>>,
    cancelled: Arc<AtomicBool>,
}

impl DownloadHandle {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        if let Ok(mut slot) = self.child.lock()
            && let Some(child) = slot.as_mut()
        {
            let _ = child.kill();
        }
    }
}

impl Drop for DownloadHandle {
    fn drop(&mut self) {
        if let Ok(mut slot) = self.child.lock()
            && let Some(child) = slot.as_mut()
        {
            let _ = child.kill();
        }
    }
}

pub fn start(
    tools: &Tools,
    url: &str,
    codec: Codec,
    height: Option<u32>,
    output_dir: &Path,
    auth: Auth<'_>,
) -> Result<DownloadHandle, String> {
    let Auth {
        cookies,
        js_runtime,
        mode,
    } = auth;
    let yt_dlp = tools
        .yt_dlp
        .as_ref()
        .ok_or_else(|| "未找到 yt-dlp，请先安装依赖".to_string())?;
    let ffmpeg = tools
        .ffmpeg
        .as_ref()
        .ok_or_else(|| "未找到 ffmpeg，请先安装依赖".to_string())?;

    let print_file = std::env::temp_dir().join(format!(
        "ymp4-output-{}-{}.txt",
        std::process::id(),
        SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or_default()
    ));

    let mut command = Command::new(yt_dlp);
    command
        .arg("--newline")
        .arg("--no-color")
        .arg("--encoding")
        .arg("utf-8")
        .arg("--no-playlist")
        .arg("--progress")
        .arg("--no-simulate")
        .arg("--windows-filenames")
        .arg("--force-overwrites")
        .arg("--merge-output-format")
        .arg("mp4")
        .arg("--remux-video")
        .arg("mp4")
        .arg("--print-to-file")
        .arg(format!("after_move:{OUTPUT_MARKER}%(filepath)s"))
        .arg(&print_file)
        .arg("-f")
        .arg(match mode {
            AuthMode::Default => codec.format_selector(height),
            AuthMode::Compatible => codec.format_selector(None),
        })
        .arg("--ffmpeg-location")
        .arg(ffmpeg)
        .arg("-P")
        .arg(output_dir)
        .arg("-o")
        .arg("%(title).70B [%(id)s].%(ext)s");

    match mode {
        AuthMode::Default => crate::ytdlp::apply_auth(&mut command, cookies, js_runtime),
        AuthMode::Compatible => {
            crate::ytdlp::apply_compatible_auth(&mut command, cookies, js_runtime)
        }
    }

    command
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }

    let mut child = command
        .spawn()
        .map_err(|e| format!("无法启动 yt-dlp：{e}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "无法读取 yt-dlp 输出".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "无法读取 yt-dlp 错误输出".to_string())?;

    let child_slot = Arc::new(Mutex::new(Some(child)));
    let cancelled = Arc::new(AtomicBool::new(false));
    let (event_tx, event_rx) = mpsc::channel();
    let (line_tx, line_rx) = mpsc::channel::<String>();

    spawn_line_reader(stdout, line_tx.clone());
    spawn_line_reader(stderr, line_tx);

    let driver_slot = Arc::clone(&child_slot);
    let driver_cancelled = Arc::clone(&cancelled);
    let driver_tx = event_tx;
    let driver_output_dir = output_dir.to_path_buf();
    let driver_print_file = print_file;
    let started_at = SystemTime::now();

    thread::spawn(move || {
        let mut last_error: Option<String> = None;

        for line in line_rx {
            handle_line(&line, &driver_tx, &mut last_error);
        }

        let status = {
            let mut slot = driver_slot.lock().unwrap_or_else(|err| err.into_inner());
            slot.as_mut().and_then(|child| child.wait().ok())
        };

        let printed_path = read_printed_path(&driver_print_file).filter(|path| path.is_file());
        let _ = fs::remove_file(&driver_print_file);

        if driver_cancelled.load(Ordering::SeqCst) {
            let _ = driver_tx.send(DownloadEvent::Cancelled);
            return;
        }

        match status {
            Some(status) if status.success() => {
                let path = printed_path.or_else(|| newest_mp4(&driver_output_dir, started_at));
                match path {
                    Some(path) => {
                        let _ = driver_tx.send(DownloadEvent::Done(path));
                    }
                    None => {
                        let _ = driver_tx.send(DownloadEvent::Failed(
                            "下载已完成，但未能定位生成的 MP4 文件".into(),
                        ));
                    }
                }
            }
            Some(status) => {
                let message = last_error.unwrap_or_else(|| match status.code() {
                    Some(code) => format!("yt-dlp 退出码：{code}"),
                    None => "yt-dlp 异常退出".into(),
                });
                let _ = driver_tx.send(DownloadEvent::Failed(message));
            }
            None => {
                let message = last_error.unwrap_or_else(|| "无法获取 yt-dlp 退出状态".into());
                let _ = driver_tx.send(DownloadEvent::Failed(message));
            }
        }
    });

    Ok(DownloadHandle {
        events: event_rx,
        child: child_slot,
        cancelled,
    })
}

fn spawn_line_reader<R: Read + Send + 'static>(reader: R, tx: Sender<String>) {
    thread::spawn(move || {
        let mut reader = BufReader::new(reader);
        let mut buffer = Vec::new();
        loop {
            buffer.clear();
            match reader.read_until(b'\n', &mut buffer) {
                Ok(0) => break,
                Ok(_) => {
                    let line = String::from_utf8_lossy(&buffer)
                        .trim_end_matches(['\r', '\n'])
                        .to_string();
                    if line.is_empty() {
                        continue;
                    }
                    if tx.send(line).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });
}

fn handle_line(line: &str, tx: &Sender<DownloadEvent>, last_error: &mut Option<String>) {
    if let Some(percent) = parse_download_percent(line) {
        let _ = tx.send(DownloadEvent::Progress {
            progress: (percent / 100.0).clamp(0.0, 0.99),
            stage: "正在下载媒体流…".into(),
        });
        return;
    }

    let stage = if line.contains("Merging formats into") || line.contains("[Merger]") {
        Some("正在合并音视频…")
    } else if line.contains("Remuxing") || line.contains("[VideoRemuxer]") {
        Some("正在转换为 MP4…")
    } else if line.contains("Deleting original file") {
        Some("正在整理输出文件…")
    } else if line.contains("[download] Destination:") {
        Some("正在下载媒体流…")
    } else if line.contains("has already been downloaded") {
        Some("文件已存在，正在处理…")
    } else {
        None
    };

    if let Some(stage) = stage {
        let _ = tx.send(DownloadEvent::Stage(stage.into()));
        if matches!(stage, "正在合并音视频…" | "正在转换为 MP4…") {
            let _ = tx.send(DownloadEvent::Progress {
                progress: 0.99,
                stage: stage.into(),
            });
        }
    }

    if line.contains("ERROR:") {
        *last_error = Some(line.trim().to_string());
    }

    let _ = tx.send(DownloadEvent::Log(line.to_string()));
}

fn read_printed_path(path: &Path) -> Option<PathBuf> {
    let contents = fs::read_to_string(path).ok()?;
    contents.lines().rev().find_map(|line| {
        let path = line.trim().strip_prefix(OUTPUT_MARKER)?.trim();
        (!path.is_empty()).then(|| PathBuf::from(path))
    })
}

fn parse_download_percent(line: &str) -> Option<f32> {
    let rest = line.strip_prefix("[download]")?.trim_start();
    let token = rest.split_whitespace().next()?;
    let percent = token.strip_suffix('%')?.parse::<f32>().ok()?;
    percent.is_finite().then_some(percent)
}

fn newest_mp4(dir: &Path, since: SystemTime) -> Option<PathBuf> {
    let threshold = since.checked_sub(Duration::from_secs(5)).unwrap_or(since);

    fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if !path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("mp4"))
            {
                return None;
            }
            let modified = entry.metadata().ok()?.modified().ok()?;
            (modified >= threshold).then_some((modified, path))
        })
        .max_by_key(|(modified, _)| *modified)
        .map(|(_, path)| path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_download_percent_lines() {
        assert_eq!(
            parse_download_percent("[download]  42.5% of 12.3MiB at 2.1MiB/s"),
            Some(42.5)
        );
        assert_eq!(
            parse_download_percent("[download] 100% of 5.00MiB in 00:00:01"),
            Some(100.0)
        );
        assert_eq!(
            parse_download_percent("[download] Destination: clip.mp4"),
            None
        );
    }

    #[test]
    fn codec_selectors_prefer_requested_codec() {
        assert!(Codec::H264.format_selector(None).contains("avc1"));
        assert!(Codec::H265.format_selector(None).contains("hev1"));
        assert!(Codec::Av1.format_selector(None).contains("av01"));
    }

    #[test]
    fn codec_selectors_filter_by_height() {
        for codec in Codec::ALL {
            let selector = codec.format_selector(Some(1440));
            assert!(selector.contains("[height=1440]"));
            assert!(!selector.contains("[height<=1440]"));
        }
        assert!(!Codec::Av1.format_selector(None).contains("[height="));
    }
}
