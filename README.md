# DeepSeek Harness Desktop

将官方 [DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness) Web 工作台变成可从 Windows 图标直接启动的桌面应用。

它不重新实现 Harness，也不复制 Codex 前端。桌面壳负责 Workspace 恢复、Runtime 生命周期、托盘、日志和 Windows 安装体验；任务、对话、工具、审批、Skills、子代理和设置仍由官方 `dsh web` 提供。

> 本项目是社区桌面客户端，不代表 DeepSeek 官方背书。DeepSeek Harness 当前仍处于 Developer Preview，可能发生破坏性变化。

> “DeepSeek”名称及鲸鱼标志归其各自权利人所有；本项目中的兼容性描述和标志展示不表示官方授权、合作或认可。

## 两个 Edition

| Edition | 适合谁 | 需要什么 | 本机实测安装包 / 便携 ZIP |
|---|---|---|---:|
| **Lite** | 已有开发环境的代码用户 | Node.js `^22.19.0` 或 `>=24.0.0`、npm、网络（仅首次自动准备 Runtime）；WebView2 | 约 1 MiB |
| **Full** | 暂停发布；保留源码以便后续继续验证 | WebView2；缺失时安装器自动补齐 | 当前不提供下载 |

两个 Edition 使用相同的 Tauri 桌面壳、相同界面和相同用户数据。安装另一 Edition 会替换当前 Edition，但保留 Workspace、Task 和设置。

> **当前发布状态：** v0.1.2 只发布 Lite。Full 的源码和兼容性编译检查仍然保留，但在完成新的实机验证前不会构建、上传或作为 v0.1.2 Release 资产提供。v0.1.1 Release 已因 Lite 启动问题撤回。

> Full 的历史实测值基于 0.1.1、Windows x64、Node 24.18.0 和 dsh 0.1.0-rc.6；后续依赖变化会影响体积。

## 已实现

- 双击图标启动官方 Harness 工作台
- 自动恢复上次 Workspace，首次启动显示文件夹选择器
- 稳定的 loopback Origin，仅监听 `127.0.0.1`
- 解析官方 stdout 就绪信号后才打开 UI
- Lite：自定义入口 → 兼容的全局 dsh → 自动安装并持久化的固定 Runtime
- Full：携带固定版本 Node.js 与 `@deepseek-ai/dsh`
- 单实例、系统托盘、登录启动选项
- 标准输出/错误输出管道监控，关闭时通过 Job Object 清理完整进程树
- Runtime 首次崩溃自动恢复，重复崩溃停止重试
- 最多 20 MB、保留 7 天的本地诊断日志
- 远程 Harness 页面不获得任何 Tauri IPC 权限
- Lite/Full 安装版、便携版和 SHA-256 的 GitHub Release 流水线

## 从源码运行

构建桌面程序需要 Rust stable、Microsoft Visual C++ Build Tools、Node.js 24 和 WebView2。最终用户不需要安装 Rust 或 Visual Studio。

**PowerShell：**

```powershell
npm.cmd install
npm.cmd run desktop:dev
```

首次打开后选择 Workspace。Lite 会优先寻找全局安装的兼容 dsh；找不到时，应用会根据随桌面版本提供的完整锁文件自动安装固定 Runtime，并在校验成功后原子切换到持久化的本地副本。首次准备可能持续数分钟并短时占用较多内存；以后双击会直接使用该副本，无需手动运行 `npx`。官方 dsh 更新只会随经过测试的新桌面版本切换，不会在启动时漂移。

## 构建 Lite

**PowerShell：**

```powershell
npm.cmd run build:lite
npm.cmd run stage:lite
```

## 构建 Full

Full 构建会下载固定的便携 Node.js，并在忽略目录中安装固定版本 Harness Runtime。

**PowerShell：**

```powershell
npm.cmd run build:full
npm.cmd run stage:full
npm.cmd run package:local
```

安装包位于 `src-tauri/target/release/bundle/nsis/`，便携目录位于 `release/`。

## 验证

**PowerShell：**

```powershell
npm.cmd run check
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml
cargo check --manifest-path src-tauri/Cargo.toml --features full-runtime
```

## 发布

推送 `v*` 标签后，GitHub Actions 当前只构建 Windows x64 Lite：

- `DeepSeek-Harness-Desktop-Lite-x64-Setup.exe`
- `DeepSeek-Harness-Desktop-Lite-x64-Portable.zip`
- `SHA256SUMS.txt`

Full 的发布任务暂时停用；恢复前必须完成规定的 Full 实机启动测试，并同步更新本文档与发布工作流。

首批 `.exe` 可以未签名发布，因此 Windows SmartScreen 可能显示“未知发布者”。项目预留免费开源签名流程，但不依赖收费证书。详见 [发布说明](docs/RELEASING.md)。

## 文档

- [架构](docs/ARCHITECTURE.md)
- [领域词汇](CONTEXT.md)
- [架构决策](docs/adr/)
- [发布流程](docs/RELEASING.md)
- [安全策略](SECURITY.md)
- [贡献指南](CONTRIBUTING.md)

## License

[MIT](LICENSE)
