@echo off
chcp 65001 >nul
title FreePatch 补丁打包工具

:: 获取脚本所在目录
set "SCRIPT_DIR=%~dp0"

:: 查找可执行文件（优先同级目录，再找 release 目录）
if exist "%SCRIPT_DIR%free-patch-gui.exe" (
    set "EXE=%SCRIPT_DIR%free-patch-gui.exe"
) else if exist "%SCRIPT_DIR%rust\target\release\free-patch-gui.exe" (
    set "EXE=%SCRIPT_DIR%rust\target\release\free-patch-gui.exe"
) else (
    echo 错误：找不到 free-patch-gui.exe
    echo 请先运行 cargo build --release 编译
    pause
    exit /b 1
)

echo.
echo  ========================================
echo    FreePatch 补丁打包工具
echo  ========================================
echo.
"%EXE%"

echo.
pause
