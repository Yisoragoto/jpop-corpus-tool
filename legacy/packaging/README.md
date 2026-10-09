# Windows 打包（0.1.x，PyQt 版）

这是 0.1.x 的打包脚本。0.2.0 起的安装包由 `app/` 里的 `npm run app:build` 构建，和这里无关。

在仓库根目录执行：

```powershell
.\legacy\packaging\build_windows.ps1 -Version 0.1.0
```

如果 `ffmpeg.exe` 不在 `PATH` 中，可显式指定：

```powershell
.\legacy\packaging\build_windows.ps1 -Version 0.1.0 -FfmpegPath "D:\tools\ffmpeg\bin\ffmpeg.exe"
```

默认同时输出便携压缩包和安装程序：

```text
legacy/dist/JpopCorpusTool-0.1.0-windows-portable.zip
legacy/dist/JpopCorpusTool-0.1.0-windows-setup.exe
```

需要 Python 3.12、Inno Setup 6，以及可选的 `ffmpeg.exe`。脚本会把 Python、PyQt6、GiNZA、字体等运行依赖打包进程序；普通用户不需要安装开发环境。

只构建便携版时可增加 `-SkipInstaller`。不要运行 `build/` 中的文件，该目录只是 PyInstaller 中间产物。
