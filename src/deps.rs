use std::{
    env, fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::mpsc::Sender,
    thread,
    time::{Duration, Instant},
};

use zip::ZipArchive;

pub const YTDLP_URL: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe";
pub const FFMPEG_URL: &str = "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip";
pub const QJS_URL: &str =
    "https://github.com/quickjs-ng/quickjs/releases/latest/download/qjs-windows-x86_64.exe";

const BUNDLED_TOOLS: &[u8] = include_bytes!("../assets/tools.zip");

#[derive(Clone, Debug, Default)]
pub struct Tools {
    pub yt_dlp: Option<PathBuf>,
    pub ffmpeg: Option<PathBuf>,
    pub js_runtime: Option<PathBuf>,
}

impl Tools {
    pub fn ready(&self) -> bool {
        self.yt_dlp.is_some() && self.ffmpeg.is_some()
    }
}

#[derive(Debug)]
pub enum InstallEvent {
    Status(String),
    Progress {
        label: &'static str,
        downloaded: u64,
        total: Option<u64>,
    },
    Finished(Result<Tools, String>),
}

pub fn app_bin_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(env::temp_dir)
        .join("YoutubeMp4Downloader")
        .join("bin")
}

pub fn detect_tools() -> Tools {
    let bin_dir = app_bin_dir();
    Tools {
        yt_dlp: find_tool("yt-dlp.exe", &bin_dir),
        ffmpeg: find_tool("ffmpeg.exe", &bin_dir),
        js_runtime: find_tool("qjs.exe", &bin_dir),
    }
}

pub fn ensure_bundled_tools() -> Result<(), String> {
    let bin_dir = app_bin_dir();
    let has_yt_dlp = bin_dir.join("yt-dlp.exe").is_file();
    let has_ffmpeg = bin_dir.join("ffmpeg.exe").is_file();
    let has_qjs = bin_dir.join("qjs.exe").is_file();
    if has_yt_dlp && has_ffmpeg && has_qjs {
        return Ok(());
    }

    fs::create_dir_all(&bin_dir)
        .map_err(|e| format!("无法创建依赖目录 {}：{e}", bin_dir.display()))?;
    let mut archive = ZipArchive::new(io::Cursor::new(BUNDLED_TOOLS))
        .map_err(|e| format!("内置依赖包无效：{e}"))?;

    for (name, present) in [
        ("yt-dlp.exe", has_yt_dlp),
        ("ffmpeg.exe", has_ffmpeg),
        ("qjs.exe", has_qjs),
    ] {
        if present {
            continue;
        }
        let destination = bin_dir.join(name);
        let temp_path = destination.with_extension("part");
        let mut entry = archive
            .by_name(name)
            .map_err(|e| format!("内置依赖包缺少 {name}：{e}"))?;
        let mut output = fs::File::create(&temp_path)
            .map_err(|e| format!("无法写入 {}：{e}", temp_path.display()))?;
        io::copy(&mut entry, &mut output).map_err(|e| format!("解压 {name} 失败：{e}"))?;
        output
            .flush()
            .map_err(|e| format!("解压 {name} 失败：{e}"))?;
        drop(output);

        if destination.exists() {
            let _ = fs::remove_file(&destination);
        }
        fs::rename(&temp_path, &destination).map_err(|e| format!("无法保存 {name}：{e}"))?;
    }

    Ok(())
}

fn find_tool(name: &str, bin_dir: &Path) -> Option<PathBuf> {
    let managed = bin_dir.join(name);
    if managed.is_file() {
        return Some(managed);
    }

    if let Ok(exe) = env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let beside_exe = dir.join(name);
        if beside_exe.is_file() {
            return Some(beside_exe);
        }
    }

    let path_var = env::var_os("PATH")?;
    env::split_paths(&path_var)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

pub fn start_install(tools: Tools, force: bool, tx: Sender<InstallEvent>) {
    let need_yt_dlp = force || tools.yt_dlp.is_none();
    let need_ffmpeg = force || tools.ffmpeg.is_none();
    let need_qjs = force || tools.js_runtime.is_none();

    thread::spawn(move || {
        let result = (|| -> Result<Tools, String> {
            let bin_dir = app_bin_dir();
            fs::create_dir_all(&bin_dir)
                .map_err(|e| format!("无法创建依赖目录 {}：{e}", bin_dir.display()))?;

            if need_yt_dlp {
                let _ = tx.send(InstallEvent::Status("正在下载 yt-dlp…".into()));
                download(YTDLP_URL, &bin_dir.join("yt-dlp.exe"), "yt-dlp", &tx)?;
            }

            if need_ffmpeg {
                let _ = tx.send(InstallEvent::Status("正在下载 ffmpeg…".into()));
                let zip_path = bin_dir.join("ffmpeg-download.zip");
                download(FFMPEG_URL, &zip_path, "ffmpeg", &tx)?;
                let _ = tx.send(InstallEvent::Status("正在解压 ffmpeg…".into()));
                extract_ffmpeg(&zip_path, &bin_dir)?;
                let _ = fs::remove_file(&zip_path);
            }

            if need_qjs {
                let _ = tx.send(InstallEvent::Status("正在下载 QuickJS…".into()));
                download(QJS_URL, &bin_dir.join("qjs.exe"), "QuickJS", &tx)?;
            }

            Ok(detect_tools())
        })();

        let _ = tx.send(InstallEvent::Finished(result));
    });
}

fn download(
    url: &str,
    destination: &Path,
    label: &'static str,
    tx: &Sender<InstallEvent>,
) -> Result<(), String> {
    let response = ureq::get(url)
        .header("User-Agent", "youtube-mp4-downloader/0.1")
        .call()
        .map_err(|e| format!("{label} 下载失败：{e}"))?;

    if !response.status().is_success() {
        return Err(format!("{label} 下载失败：HTTP {}", response.status()));
    }

    let total = response.body().content_length();
    let mut reader = response.into_body().into_reader();
    let temp_path = destination.with_extension("part");
    let mut file = fs::File::create(&temp_path)
        .map_err(|e| format!("无法写入 {}：{e}", temp_path.display()))?;

    let mut buffer = [0u8; 64 * 1024];
    let mut downloaded: u64 = 0;
    let mut last_report = Instant::now()
        .checked_sub(Duration::from_secs(1))
        .unwrap_or_else(Instant::now);

    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|e| format!("{label} 下载中断：{e}"))?;
        if read == 0 {
            break;
        }
        file.write_all(&buffer[..read])
            .map_err(|e| format!("{label} 写入磁盘失败：{e}"))?;
        downloaded += read as u64;

        if last_report.elapsed() >= Duration::from_millis(150) {
            let _ = tx.send(InstallEvent::Progress {
                label,
                downloaded,
                total,
            });
            last_report = Instant::now();
        }
    }

    file.flush()
        .map_err(|e| format!("{label} 写入磁盘失败：{e}"))?;
    drop(file);

    if let Some(expected) = total
        && downloaded < expected
    {
        let _ = fs::remove_file(&temp_path);
        return Err(format!(
            "{label} 下载不完整（{downloaded} / {expected} 字节）"
        ));
    }

    if destination.exists() {
        fs::remove_file(destination)
            .map_err(|e| format!("无法覆盖旧文件 {}：{e}", destination.display()))?;
    }
    fs::rename(&temp_path, destination)
        .map_err(|e| format!("无法保存 {}：{e}", destination.display()))?;

    let _ = tx.send(InstallEvent::Progress {
        label,
        downloaded,
        total: total.or(Some(downloaded)),
    });
    Ok(())
}

fn extract_ffmpeg(zip_path: &Path, bin_dir: &Path) -> Result<(), String> {
    let file = fs::File::open(zip_path).map_err(|e| format!("无法打开 ffmpeg 压缩包：{e}"))?;
    let mut archive = ZipArchive::new(file).map_err(|e| format!("ffmpeg 压缩包无效：{e}"))?;

    let mut extracted_any = false;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|e| format!("读取 ffmpeg 压缩包失败：{e}"))?;
        if entry.is_dir() {
            continue;
        }

        let Some(entry_path) = entry.enclosed_name() else {
            continue;
        };
        let Some(file_name) = entry_path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !matches!(file_name, "ffmpeg.exe" | "ffprobe.exe") {
            continue;
        }

        let destination = bin_dir.join(file_name);
        let temp_path = destination.with_extension("part");
        let mut output = fs::File::create(&temp_path)
            .map_err(|e| format!("无法写入 {}：{e}", temp_path.display()))?;
        io::copy(&mut entry, &mut output).map_err(|e| format!("解压 {file_name} 失败：{e}"))?;
        output
            .flush()
            .map_err(|e| format!("解压 {file_name} 失败：{e}"))?;
        drop(output);

        if destination.exists() {
            let _ = fs::remove_file(&destination);
        }
        fs::rename(&temp_path, &destination).map_err(|e| format!("无法保存 {file_name}：{e}"))?;
        extracted_any = true;
    }

    if !extracted_any || !bin_dir.join("ffmpeg.exe").is_file() {
        return Err("ffmpeg 安装包中未找到 ffmpeg.exe".into());
    }
    Ok(())
}
