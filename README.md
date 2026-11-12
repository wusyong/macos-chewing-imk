# macOS 新酷音（ChewingIMK）

用純 Rust（[objc2](https://github.com/madsmtm/objc2)）透過 InputMethodKit 實作的 macOS 新酷音輸入法，引擎是 [libchewing](https://codeberg.org/chewing/libchewing)。可以用注音打字、選字，單按 Shift 切換中英，有候選窗、輸入法選單和偏好設定視窗。目前是個人專案，只供自己安裝使用。
## 需求

- macOS 14 Sonoma 以上
- Rust：版本由 `rust-toolchain.toml` 固定，rustup 會自動安裝
- Xcode Command Line Tools（提供 `codesign`、`ditto`）

## 開發

```bash
cargo xtask install   # 編譯、組裝 .app、裝到 ~/Library/Input Methods 並註冊
cargo xtask log       # 即時查看輸入法的 os_log 輸出（另開一個終端機）
```

裝好後，從選單列的輸入法選單選「新酷音」。如果清單裡還沒出現，登出再登入一次。

其他指令：

```bash
cargo xtask test        # 下載字典並跑測試
cargo xtask bundle      # 只組裝 target/debug/ChewingIMK.app
cargo xtask uninstall   # 停止並移除輸入法
```

每個指令都可以加上 `--release`。

CI（`.github/workflows/ci.yml`）在 GitHub Actions 上跑 `cargo fmt --check`、`cargo clippy`、`cargo xtask test` 和 `cargo xtask bundle --release`。

### 專案結構

| 路徑 | 內容 |
|---|---|
| `crates/chewing_imk` | 輸入法行程：`IMKServer`、`ChewingController`、AppKit UI |
| `crates/chewing_imk_core` | 平台無關的按鍵處理狀態機，不依賴 objc2 |
| `crates/chewing_imk/resources` | `Info.plist` 範本與選單列圖示 |
| `xtask` | 組裝、安裝、log 等開發指令 |

## 授權

GPL-3.0-or-later，見 [COPYING](COPYING)。
