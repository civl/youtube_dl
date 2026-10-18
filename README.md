# YouTube MP4 下载器

一个使用 Rust + egui 构建的 Windows 桌面应用：粘贴 YouTube 链接，选择视频编码与清晰度，程序调用内置的 yt-dlp 与 ffmpeg 完成下载、音视频合并和 MP4 转封装，并保存到指定文件夹。需要登录的内容可通过「低隐私模式」加载浏览器导出的 cookies.txt 访问；该模式内置 QuickJS 运行时，用于求解 YouTube 的 JS 挑战。

发布版为自包含的单文件程序，内置依赖随可执行文件分发，首次启动无需联网安装。

## 功能概览

- 支持 `youtube.com`、`youtu.be`、`youtube-nocookie.com` 及子域名的 http/https 链接，并拒绝仿冒域名。
- 三种目标编码：H.264（兼容性最佳）、H.265（更小体积）、AV1（最高压缩率）。
- 按编码解析源视频实际可用的清晰度列表，下拉选择；默认「最高可用」。
- 低隐私模式：加载 Netscape 格式 cookies.txt 后，解析与下载均使用该 cookie 访问登录可见或年龄验证内容；默认关闭。该模式同时启用内置 QuickJS 运行时与登录兼容客户端列表，规避 yt-dlp 当前在登录态下选择失效客户端的问题。
- 保存目录默认取系统下载文件夹，可通过文本框修改或系统文件夹选择器指定。
- 下载进度、阶段提示、运行日志、错误信息集中在界面展示，支持取消下载。
- 内置 yt-dlp / ffmpeg / QuickJS，运行时自动展开；另提供「升级依赖」在线获取最新版。

## 技术栈

| 组件 | 版本 | 用途 |
| --- | --- | --- |
| Rust | edition 2024 | 应用语言 |
| eframe / egui | 0.37 | 原生窗口与即时模式 GUI |
| rfd | 0.17 | 系统原生文件夹选择对话框 |
| yt-dlp | 随包内置 | 解析视频信息、下载媒体流、调用 ffmpeg 合并 |
| ffmpeg | 随包内置 | 音视频合并与 MP4 转封装 |
| QuickJS-ng | 随包内置 | 低隐私模式下求解 YouTube JS 挑战（`n` 参数） |
| serde_json | 1 | 解析 yt-dlp 输出的视频元数据 JSON |
| ureq | 3.4 | 同步 HTTP 下载依赖升级包（Windows 下启用系统代理） |
| zip | 9.0 | 读取内置依赖包、解压 ffmpeg 发行包 |
| dirs | 7.0 | 定位系统下载目录与本地数据目录 |

## 模块结构

```text
youtube_dl/
├─ Cargo.toml
├─ build.rs              # Windows executable icon resource
├─ assets/
│  ├─ youtube.ico        # Windows Explorer / executable icon
│  ├─ tools.zip          # 编译期嵌入的 yt-dlp.exe + ffmpeg.exe + qjs.exe
│  └─ qjs-LICENSE.txt    # 内置 QuickJS-ng 的 MIT 许可证
└─ src/
   ├─ main.rs            # 入口、Windows 子系统配置、CJK 字体加载、窗口创建
   ├─ app.rs             # 全部 UI、应用状态（含低隐私模式）、后台事件消费与任务调度
   ├─ ytdlp.rs           # 登录态共用的 yt-dlp 认证参数（cookies / QuickJS / 客户端列表）
   ├─ probe.rs           # 调用 yt-dlp 获取元数据，按编码提取清晰度
   ├─ downloader.rs      # 构建 yt-dlp 命令、格式选择、进度解析、取消与结果定位
   └─ deps.rs            # 内置依赖展开、工具探测、在线下载与升级
```

## 启动流程

1. **Windows 子系统**：`main.rs` 顶部声明
   `#![cfg_attr(all(windows, not(test)), windows_subsystem = "windows")]`。
   普通 Debug/Release 构建均以 GUI 子系统启动，不创建控制台窗口；仅 `cargo test` 的测试程序保留控制台以便输出结果。非 Windows 平台不应用该属性。
2. **字体**：`install_cjk_font` 依次尝试微软雅黑、等线、黑体、宋体等系统字体（以及 macOS/Linux 的 CJK 字体），读取成功即注册为 egui 主字体，保证中文界面正常显示。
3. **主题**：`app::apply_theme` 设置深色视觉、圆角、强调色与文本字号。
4. **依赖准备**：`deps::ensure_bundled_tools()` 检查 `%LOCALAPPDATA%\YoutubeMp4Downloader\bin`，缺失文件从嵌入的 `assets/tools.zip` 直接解压，不发起网络请求。
5. **工具探测**：`deps::detect_tools()` 解析 yt-dlp、ffmpeg 与 QuickJS（`qjs.exe`）的最终路径。
6. **进入事件循环**：`eframe::run_native` 创建 820×860 窗口（最小 660×560）。

## 依赖管理逻辑

### 内置依赖展开

`deps.rs` 通过 `include_bytes!("../assets/tools.zip")` 将依赖包编译进可执行文件。

- 目标目录：`%LOCALAPPDATA%\YoutubeMp4Downloader\bin`（回退到系统临时目录）。
- 启动时只展开缺失的 `yt-dlp.exe`、`ffmpeg.exe` 与 `qjs.exe`；已存在则完全跳过。
- 解压写入 `.part` 临时文件，刷新后原子重命名，避免中断产生半成品。
- zip 内只包含上述三个可执行文件，不包含 `ffprobe.exe`。

### 工具查找顺序

`find_tool` 对每个工具依次检查：

1. 应用托管目录（上面的 `bin`），升级后的版本优先生效；
2. 可执行文件同目录；
3. 系统 `PATH`。

### 在线安装与升级

- 「安装缺失依赖」仅在检测到缺失时出现；「升级依赖」在 yt-dlp 与 ffmpeg 就绪时出现，强制重新下载最新版。
- 下载源：yt-dlp 官方 GitHub `releases/latest`，ffmpeg 使用 gyan.dev essentials 发行包，QuickJS 使用 quickjs-ng 官方 GitHub 发布页的 `qjs-windows-x86_64.exe`。
- 升级在后台线程通过阻塞式 `ureq` 执行，每 150 ms 上报已下载字节数与总大小，界面显示进度条。
- 文件先写 `.part`，长度校验通过后删除旧文件并重命名；ffmpeg 压缩包仅提取 `ffmpeg.exe` / `ffprobe.exe`，使用 `enclosed_name()` 防御路径穿越，完成后删除压缩包。
- QuickJS-ng 以 MIT 许可证分发，许可证文本随仓库保存在 `assets/qjs-LICENSE.txt`。
- Windows 下启用 `win-system-proxy` 特性，自动遵循系统代理设置。

## 清晰度解析

`probe.rs` 在后台线程执行：

```text
yt-dlp --dump-single-json --no-playlist --no-warnings --no-color
       --skip-download --encoding utf-8
       [--cookies <cookies.txt> --js-runtimes quickjs:<qjs.exe>
        --extractor-args "youtube:player_client=default,-tv_downgraded"]
       <URL>
```

解析 JSON 中的 `formats` 数组：

- 跳过 `vcodec == "none"`（纯音频）与缺失高度、或高度不在 1–8640 范围内的条目。
- 按编码前缀归类：
  - `avc1…` → H.264 列表
  - `hev1…` / `hvc1…` → H.265 列表
  - `av01…` → AV1 列表
- 所有高度合并进总列表；各列表降序排序并去重。

界面行为：

- 清晰度下拉框只显示当前所选编码实际可用的高度，例如源视频 4K 仅有 AV1 时，H.264 列表最高显示 1080p。
- 默认值为 `None`，语义是「最高可用」，下载时使用该编码的最高质量选择链。
- 切换编码、修改 URL 会校验当前选择；若新编码不支持该高度则回落到最高可用。
- 若解析结果中当前编码无任何高度，界面提示「当前编码无可用清晰度，将自动回退」，下载选择链会退到其他编码或最佳可用流。
- 未点击解析时同样可以下载，此时按最高清晰度选择。

## 低隐私模式（cookies.txt）

需要登录、会员或年龄验证的视频，可通过用户显式提供的 cookie 文件访问：

- 在「登录内容」区域勾选「低隐私模式」，通过「选择文件…」指定 `cookies.txt`，也可直接输入路径。
- 未勾选时完全不使用 cookie；勾选后若路径为空或文件不存在，任务会在启动前直接报错。
- 勾选后若缺少 QuickJS 运行时，任务同样会在启动前报错，并提示通过底部「升级依赖」补齐。
- 解析清晰度与下载共用该设置，两个 yt-dlp 调用都会附加：

```text
--cookies <cookies.txt>
--js-runtimes quickjs:<qjs.exe>
--extractor-args "youtube:player_client=default,-tv_downgraded"
```

- 登录态下 yt-dlp 目前会自动选择已知失效的 `tv_downgraded` 客户端，导致 `The page needs to be reloaded.`。上述参数沿用 yt-dlp 的默认客户端列表并显式排除 `tv_downgraded`；固定只使用 `web_safari` / `web_embedded` 时可能被 YouTube 的 SABR 实验限制为低清，因此不采用。同时 YouTube 的 `n` 参数需要 JS 运行时求解，否则只能拿到低清格式，因此必须提供 QuickJS。
- 以上三个参数只在低隐私模式下添加；普通公开视频的解析与下载命令保持不变。
- 若登录态客户端返回的高清直链被 YouTube 以 `HTTP 403` 拒绝（缺少 PO Token 或 SABR 实验限制），应用会自动切换到 `web_embedded` 兼容模式重试一次。兼容模式通常只能获取 360p 左右的渐进式格式，但可避免整个任务直接失败；界面日志会注明发生了降级。
- 应用只校验路径是否存在并传递给 yt-dlp，**不读取、不解析文件内容，不复制、不上传**；cookie 内容仅由本机 yt-dlp 进程读取。
- 推荐使用 Chrome 扩展 [Get cookies.txt LOCALLY](https://chromewebstore.google.com/detail/get-cookiestxt-locally/cclelndahbckbenkjhflpdbgdldlbecc) 导出 Netscape 格式文件。
- cookies 等同于账号登录凭证：仅导出自己的账号，不要分享文件或提交到版本库；使用完毕后可在浏览器退出登录以使会话失效。
- 若导出的文件包含其他站点 cookie，建议在导出前仅保留 YouTube / Google 域名，或使用一次性浏览器配置完成导出。

## 下载与转码

### 命令构建

`downloader::start` 以子进程方式调用 yt-dlp，关键参数：

```text
--newline --no-color --encoding utf-8 --no-playlist --progress --no-simulate
--windows-filenames --force-overwrites
--merge-output-format mp4 --remux-video mp4
--print-to-file after_move:__YMP4_OUTPUT__%(filepath)s <临时文件>
-f <格式选择器>
--ffmpeg-location <ffmpeg.exe>
-P <保存目录> -o %(title).70B [%(id)s].%(ext)s
[--cookies <cookies.txt>
 --js-runtimes quickjs:<qjs.exe>
 --extractor-args "youtube:player_client=default,-tv_downgraded"]
<URL>
```

- `--merge-output-format mp4` 保证音视频分离流合并为 MP4；`--remux-video mp4` 对单文件流做 MP4 转封装。
- `--force-overwrites` 确保切换清晰度后同名文件会被重新生成，而不是被 yt-dlp 跳过。
- `--windows-filenames` 过滤文件名非法字符；输出模板限制标题长度并附加视频 ID，降低重名概率。
- Windows 下为子进程设置 `CREATE_NO_WINDOW`（`0x0800_0000`），避免弹出控制台。

### 格式选择器

每个编码对应一条由 `/` 分隔的优先级回退链。未指定高度时使用该编码最高质量组合（如 H.264 优先 `avc1` + `m4a` 的 MP4 流）；指定高度时每一级都附带精确的 `[height=H]` 过滤，例如 H.264：

```text
bv*[height=1080][vcodec^=avc1][ext=mp4]+ba[ext=m4a]
/ bv*[height=1080][vcodec^=avc1]+ba[ext=m4a]
/ bv*[height=1080][vcodec^=avc1]+ba
/ b[height=1080][ext=mp4][vcodec^=avc1]
/ b[height=1080][ext=mp4] / bv*[height=1080]+ba / b[height=1080]
```

逐级放宽编码与封装要求，尽量命中目标高度；若全部不命中则由 yt-dlp 回退到最佳可用流。

### 事件与进度

- stdout 与 stderr 各由一个读取线程按行拆包，统一送入行通道。
- 驱动线程消费行：从 `[download] 42.5%` 提取百分比，映射为 0–0.99 的总进度；识别 `Merging formats into`、`Remuxing`、`[Merger]`、`[VideoRemuxer]` 等关键字切换阶段文案；暂存最后一条 `ERROR:` 作为失败原因；原始行转发到 UI 日志。
- 进程结束后读取 `--print-to-file` 写入的最终路径；若文件不存在，则回退为「保存目录中下载开始后最新修改的 MP4」。
- 取消通过 `AtomicBool` 标记并 `kill` 子进程实现；`DownloadHandle` 被丢弃时也会终止未完成的子进程。

## UI 与线程模型

界面采用单 UI 线程 + 后台工作线程，无异步运行时：

```text
UI 线程（egui 每帧）
├─ probe::start         → 后台线程执行 yt-dlp 元数据解析 → ProbeEvent
├─ downloader::start    → yt-dlp 子进程 + 行读取/驱动线程      → DownloadEvent
└─ deps::start_install  → 后台线程执行 HTTP 下载与解压         → InstallEvent

poll_background() 每帧 try_recv 清空各通道 → 更新进度、日志、依赖状态
```

- 所有跨线程通信使用 `std::sync::mpsc`，任务句柄持有 `Receiver`。
- 有任务运行时调用 `Context::request_repaint_after(100ms)`，保证后台事件及时刷新。
- 下载进行中禁用编码卡片、目录选择等输入；安装或解析进行中禁用下载与升级按钮，避免任务冲突。
- 日志为最多 300 行的环形队列，默认折叠展示。
- 下载成功后可「打开文件夹」，按平台调用 `explorer` / `open` / `xdg-open`。

## 关键设计取舍

- **子进程封装 yt-dlp**：解析与下载行为跟随 yt-dlp 更新，无需自行处理 YouTube 签名与流协议；应用只负责编排与展示。
- **依赖内嵌而非首次联网**：发布包增大约 72 MiB，但保证离线可用、首次启动即可下载；在线升级作为可选能力保留。
- **按编码维护清晰度**：不同编码在源站可用高度经常不同，按编码过滤可避免「选了 4K 却下到 1080p」的误导。
- **精确高度 + 回退链**：优先满足用户选择，源站缺失时仍能产出可播放文件。
- **认证与浏览器解耦**：不直接读取 Chrome/Edge 的 cookie 数据库，也不要求安装额外组件；由用户显式提供 cookies.txt，应用只传递路径，数据边界清晰。
- **登录态显式覆盖客户端与运行时**：通过 `--extractor-args` 排除当前失效的客户端，并把内置 QuickJS 路径传给 yt-dlp，避免依赖用户机器上恰好存在的 Node.js。
- **临时文件 + 原子替换**：依赖升级和下载路径解析都尽量避免半成品与旧文件占用问题。

## 构建与运行

```bash
# 开发运行（Windows 下同样不显示控制台）
cargo run

# 发布构建
cargo build --release
# 产物：target/release/youtube_mp4_downloader.exe（自包含依赖，约 72 MiB）

# 测试与静态检查
cargo test
cargo clippy --all-targets -- -D warnings
```

> 更换 `assets/tools.zip` 后需重新编译，内置依赖才会更新。也可直接使用界面右下角的「升级依赖」。

## 已知限制

- 依赖网络访问 YouTube，受限地区需要自行配置代理（依赖升级会遵循 Windows 系统代理）。
- 不支持受 DRM 保护的内容；需要登录或年龄验证的视频可通过低隐私模式配合 cookies.txt 尝试访问。
- 登录可见内容依赖 cookie 的有效期与账号权限；过期或权限不足时需重新导出。
- 低隐私模式可能受 YouTube 的 PO Token / SABR 实验影响：高清直链被拒时只能以兼容模式输出较低清晰度。公开视频不勾选低隐私模式时不受此限制。
- 登录态客户端覆盖针对 yt-dlp 当前行为设置；上游修复后可通过「升级依赖」更新 yt-dlp，必要时再调整该列表。
- QuickJS 只在低隐私模式下传给 yt-dlp，普通公开视频不会使用该运行时。
- 实际输出编码取决于源站提供的流；目标编码缺失时自动回退。
- 清晰度为 yt-dlp 报告的视频高度，不等同于画质评分；高分辨率不一定代表更高码率。
