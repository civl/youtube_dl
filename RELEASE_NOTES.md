# YouTube MP4 下载器（Windows x64）

基于 Rust + egui 的 Windows 桌面应用：粘贴 YouTube 链接，选择编码与清晰度，调用内置的 yt-dlp / ffmpeg / QuickJS 完成下载、音视频合并与 MP4 转封装。

## 支持平台

- **操作系统**：Windows（建议 Windows 10 / 11）
- **架构**：x64（x86_64）
- 发布为**自包含单文件**（`youtube_mp4_downloader.exe`，约 72 MiB），内置全部依赖，首次启动无需联网安装；也可通过「升级依赖」在线更新。

## 主要功能

- 支持 youtube.com / youtu.be / youtube-nocookie.com 及子域名，并拒绝仿冒域名
- 三种目标编码：H.264（兼容性最佳）、H.265（更小体积）、AV1（最高压缩率）
- 按编码解析实际可用清晰度，默认「最高可用」
- 低隐私模式：加载 cookies.txt 访问登录 / 年龄验证内容，内置 QuickJS 求解 JS 挑战
- 中文界面、深色主题、实时进度与日志、可取消下载

## 使用说明

1. 下载 `youtube_mp4_downloader.exe` 并运行
2. 粘贴链接 → 选择编码 / 清晰度 → 开始下载（保存到系统下载目录，可自定义）
3. 登录可见内容：勾选「低隐私模式」并指定 cookies.txt（推荐用 Chrome 扩展 Get cookies.txt LOCALLY 导出 Netscape 格式）

## 已知限制

- 需联网访问 YouTube；受限地区请自行配置代理（升级会遵循 Windows 系统代理）
- 不支持受 DRM 保护的内容
- 清晰度为 yt-dlp 报告的视频高度，不等同于画质评分
- 详见仓库 README
