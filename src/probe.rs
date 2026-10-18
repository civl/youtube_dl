use std::{
    path::Path,
    process::{Command, Stdio},
    sync::mpsc::{self, Receiver},
    thread,
};

use serde_json::Value;

use crate::downloader::Codec;

#[derive(Clone, Debug)]
pub struct VideoInfo {
    pub title: String,
    pub heights: Heights,
}

#[derive(Clone, Debug, Default)]
pub struct Heights {
    pub all: Vec<u32>,
    pub h264: Vec<u32>,
    pub h265: Vec<u32>,
    pub av1: Vec<u32>,
}

impl Heights {
    pub fn for_codec(&self, codec: Codec) -> &[u32] {
        match codec {
            Codec::H264 => &self.h264,
            Codec::H265 => &self.h265,
            Codec::Av1 => &self.av1,
        }
    }
}

pub enum ProbeEvent {
    Finished(Result<VideoInfo, String>),
}

pub struct ProbeHandle {
    pub events: Receiver<ProbeEvent>,
}

pub fn start(
    yt_dlp: &Path,
    url: &str,
    cookies: Option<&Path>,
    js_runtime: Option<&Path>,
) -> ProbeHandle {
    let (tx, rx) = mpsc::channel();
    let yt_dlp = yt_dlp.to_path_buf();
    let url = url.to_string();
    let cookies = cookies.map(Path::to_path_buf);
    let js_runtime = js_runtime.map(Path::to_path_buf);

    thread::spawn(move || {
        let result = run(&yt_dlp, &url, cookies.as_deref(), js_runtime.as_deref());
        let _ = tx.send(ProbeEvent::Finished(result));
    });

    ProbeHandle { events: rx }
}

fn run(
    yt_dlp: &Path,
    url: &str,
    cookies: Option<&Path>,
    js_runtime: Option<&Path>,
) -> Result<VideoInfo, String> {
    let mut command = Command::new(yt_dlp);
    command
        .arg("--dump-single-json")
        .arg("--no-playlist")
        .arg("--no-warnings")
        .arg("--no-color")
        .arg("--skip-download")
        .arg("--encoding")
        .arg("utf-8");
    crate::ytdlp::apply_auth(&mut command, cookies, js_runtime);
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

    let output = command
        .output()
        .map_err(|e| format!("无法启动 yt-dlp：{e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let message = stderr
            .lines()
            .rev()
            .find(|line| line.contains("ERROR:"))
            .or_else(|| stderr.lines().rev().find(|line| !line.trim().is_empty()))
            .map(|line| line.trim().to_string())
            .unwrap_or_else(|| match output.status.code() {
                Some(code) => format!("yt-dlp 退出码：{code}"),
                None => "yt-dlp 异常退出".into(),
            });
        return Err(format!("解析失败：{message}"));
    }

    let value: Value =
        serde_json::from_slice(&output.stdout).map_err(|e| format!("无法解析视频信息：{e}"))?;
    let title = value
        .get("title")
        .and_then(Value::as_str)
        .filter(|title| !title.trim().is_empty())
        .unwrap_or("未命名视频")
        .to_string();
    let heights = collect_heights(&value);
    if heights.all.is_empty() {
        return Err("未找到可用的视频清晰度。".into());
    }

    Ok(VideoInfo { title, heights })
}

fn collect_heights(value: &Value) -> Heights {
    let Some(formats) = value.get("formats").and_then(Value::as_array) else {
        return Heights::default();
    };

    let mut heights = Heights::default();
    for format in formats {
        let Some(vcodec) = format.get("vcodec").and_then(Value::as_str) else {
            continue;
        };
        if vcodec == "none" {
            continue;
        }
        let Some(height) = format
            .get("height")
            .and_then(Value::as_u64)
            .filter(|height| (1..=8_640).contains(height))
            .map(|height| height as u32)
        else {
            continue;
        };

        heights.all.push(height);
        if vcodec.starts_with("avc1") {
            heights.h264.push(height);
        } else if vcodec.starts_with("hev1") || vcodec.starts_with("hvc1") {
            heights.h265.push(height);
        } else if vcodec.starts_with("av01") {
            heights.av1.push(height);
        }
    }

    for list in [
        &mut heights.all,
        &mut heights.h264,
        &mut heights.h265,
        &mut heights.av1,
    ] {
        list.sort_unstable_by(|a, b| b.cmp(a));
        list.dedup();
    }
    heights
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_heights_per_codec() {
        let value: Value = serde_json::from_str(
            r#"{
                "formats": [
                    {"vcodec": "avc1.640028", "height": 1080},
                    {"vcodec": "vp9", "height": 2160},
                    {"vcodec": "none", "height": 2160},
                    {"vcodec": "av01.0.08M", "height": 1080},
                    {"vcodec": "hvc1.1.6.L120.90", "height": 720},
                    {"vcodec": "avc1", "height": null}
                ]
            }"#,
        )
        .unwrap();

        let heights = collect_heights(&value);
        assert_eq!(heights.all, vec![2160, 1080, 720]);
        assert_eq!(heights.h264, vec![1080]);
        assert_eq!(heights.h265, vec![720]);
        assert_eq!(heights.av1, vec![1080]);
    }

    #[test]
    fn returns_empty_without_formats() {
        let value: Value = serde_json::from_str(r#"{"title": "clip"}"#).unwrap();
        assert!(collect_heights(&value).all.is_empty());
    }
}
