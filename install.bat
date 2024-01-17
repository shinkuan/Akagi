@echo off
SETLOCAL EnableDelayedExpansion
chcp 65001

echo 檢查 mitm.py 和 client.py 是否存在...
if not exist mitm.py (
    echo mitm.py 不存在
    exit /b
)
if not exist client.py (
    echo client.py 不存在
    exit /b
)

echo 檢查 Python 是否已安裝...
python --version >nul 2>&1
if errorlevel 1 (
    echo Python 未安裝
    exit /b
)

echo 創建 Python 虛擬環境...
python -m venv venv
if errorlevel 1 (
    echo 創建虛擬環境失敗
    exit /b
)

echo 進入 Python 虛擬環境...
CALL venv\Scripts\activate.bat

echo 檢查 requirement.txt 是否存在並安裝...
if not exist requirement.txt (
    echo requirement.txt 不存在
    exit /b
)
python -m pip install -r requirement.txt
if errorlevel 1 (
    echo 安裝 requirements 失敗
    exit /b
)

echo 檢查 mjai.app 資料夾是否存在...
if not exist mjai.app (
    echo mjai.app 資料夾不存在
    exit /b
)

echo 進入 mjai.app 資料夾...
cd mjai.app

echo 在 mjai.app 資料夾內執行安裝...
python -m pip install -e .
if errorlevel 1 (
    echo 安裝 mjai.app 失敗
    exit /b
)

echo 所有步驟完成，退出...
pause >nul
ENDLOCAL