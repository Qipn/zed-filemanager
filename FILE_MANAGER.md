# 文件管理器（Zed fork）

本仓库是 [zed-industries/zed](https://github.com/zed-industries/zed) 的 fork，在内置面板体系里
新增了一个 **GNOME Files 风格的文件管理器 Dock 面板**，代码在 `crates/file_manager`。
除该 crate 与少量集成点外，其余与上游完全一致。

设计取向：

- **外壳与交互**对齐 **GNOME Files（Nautilus）**：单行 header bar、面包屑/路径编辑/搜索
  共用一个位置槽、侧栏 Places 分组、行内联重命名
- **内容区尺寸与列**参考 **OpenXplorer** 的 UI 规格（`openxplorer/native/docs/ui-spec.md`）
- **配色全部取自 Zed 主题**，不引入第三方色板，与编辑器其它面板一致

---

## 1. 功能

### 浏览与导航

- 目录列表（`uniform_list` 虚拟化）与图标网格两种视图（`Ctrl+Shift+I` 切换）
- 详情视图四列：Name | Date modified | Type | Size，表头点击排序（升/降序箭头）
- **详情视图内就地展开目录**：目录行前 16px 箭头，子项按层级缩进，懒加载
- 历史栈：后退 / 前进 / 上级 / 刷新
- **位置槽三态共用**：面包屑 / 可编辑路径 / 搜索
  - 根段是地址栏前的文件夹图标（点击回 `/`），祖先段左键跳转、右键弹出「跳转到此目录 +
    同级目录」菜单，`…` 可点开被省略的祖先
  - 最后一段（当前目录）点击 = 进入编辑（对齐 Nautilus 的 `toolbar.edit-location`）
  - 编辑态支持 `~`、相对路径、IME、选区、`Ctrl+A/C/X/V`；`Enter` 确认 / `Esc` 取消
- **递归搜索**（`model/search.rs`）：深度上限 8、结果上限 2000，按名称不区分大小写；
  结果扁平列出，状态栏显示 `N results`（被截断时 `N+`），键入去抖 120ms
- 隐藏文件开关、按名称/修改时间/大小排序
- 右侧详情面板：预览图标、名称、类型、Open 按钮、大小/修改时间/位置

### 文件操作

- 新建文件夹 / 新建文件（`New ▾` 菜单：Folder / Text document / File / Markdown / CSV / JSON / HTML）
- **内联重命名**：新建后立即进入编辑，只选中主名（文件不含扩展名）；校验空名、`/`、
  首尾空格、同名冲突
- 复制 / 剪切 / 粘贴（面板内剪贴板，粘贴到当前目录）
- **拖放**：拖动已选中行时带上整个选区；支持从外部（文件管理器/桌面）拖入；
  落点亮色用 Zed 的 `drop_target_background`
- 删除：`Move to Trash` 走 `Fs::trash`（freedesktop 回收站，可撤销）、
  `Delete Permanently` 直接删除；两者都有确认弹窗
- **粘贴/拖放重名冲突**：面板内联提示条，可选替换 / 跳过 / 两者都保留
- **撤销 / 重做**（`Ctrl+Z` / `Ctrl+Shift+Z`）：新建、重命名、复制、移动、回收站均可回滚

### 侧栏

- **Places**（对齐 `nautilus-sidebar.c::update_places`）：Home → 快速访问（读
  `~/.config/user-dirs.dirs`，支持本地化名）→ 回收站 → 项目根 → 卷 → 网络（读 `/proc/mounts`）
  → 书签；分组之间是 1px 分隔线，重复路径去重
- **书签**：与 GTK 共用 `$XDG_CONFIG_HOME/gtk-3.0/bookmarks`，`Ctrl+D` 收藏当前目录，
  行内 `×` 取消，右键 Open / Add·Remove / Copy Path
- **文件夹树**：懒加载；**点击行只导航，展开只由 `>` 触发**；
  **树根粘滞**（在 home 内以 home 为根，之外以 `/` 为根，不因导航而重建）；
  **严格镜像当前路径**——展开到当前目录、把当前行滚进可见区、离开的分支跟着折叠
- 侧栏宽度可拖（120–720px，保证内容区 ≥200px）；Places / 树之间的比例分栏也可拖，
  双击复位；`F9` 开关侧栏

### 与 Zed 集成

- 双击文件用 Zed 打开（`Workspace::open_abs_path`）
- 右键目录 **Open in This Window**（加入当前窗口的工作区组并切换过去，旧工作区保留可切回；
  该目录已是本窗口某工作区根时直接切过去）或 **Open in New Window**
- 向编辑器/项目面板/大纲面板提供 `reveal_in_file_manager`：在 Zed 里右键文件选
  “Reveal in File Manager” 会定位到本面板（无面板时回退到系统文件管理器）

### 状态持久化

以下 UI 状态存在 Zed 的 kvp 数据库（key `file_manager_panel_state`）：
侧栏开关与宽度、Places/树分栏比例、隐藏文件开关、排序方式、列表/网格、图标尺寸、详情面板开关。
写入去抖 250ms，读取时对越界值做夹紧；**浏览的目录不持久化**（始终跟随当前项目）。

---

## 2. 交互速查

| 快捷键 | 作用 |
|---|---|
| `Ctrl+F` | 打开/关闭搜索 |
| `Ctrl+L` | 编辑路径 |
| `Enter` | 确认当前内联输入（路径/重命名）|
| `Esc` | 取消内联输入，或在搜索中退出搜索 |
| `F2` | 重命名 |
| `Ctrl+X` / `Ctrl+C` / `Ctrl+V` | 剪切 / 复制 / 粘贴（输入框有焦点时操作文本）|
| `Ctrl+A` | 全选（输入框有焦点时选中文本）|
| `Ctrl+Z` / `Ctrl+Shift+Z` | 撤销 / 重做 |
| `Ctrl+D` | 收藏/取消收藏当前目录 |
| `F9` | 开关侧栏 |
| `Ctrl+Shift+I` | 列表 / 网格视图切换 |
| `Menu` / `Shift+F10` | 打开右键菜单 |

鼠标：单击选中、双击打开、目录前箭头就地展开、点击侧栏行导航、
拖动分栏线调整宽度/比例（双击复位）。

> **已知问题**：header bar 上有三处 tooltip 宣称的快捷键其实没有绑定
> （`Ctrl+Shift+N` 实际被全局的 `workspace::NewWindow` 占用，`Alt+Left/Right`
> 未绑定）。以上表格只列真实可用的键。

---

## 3. 构建与运行

Zed 需要 **Rust 1.98.1**（`rust-toolchain.toml` 已钉住）和一批系统库。

### NixOS（推荐，本仓库自带脚本）

```bash
script/build-zed            # debug 构建
script/build-zed --check    # cargo check，最快
script/build-zed run        # 构建后启动
script/build-zed --release  # release 构建
script/build-zed --test -p file_manager   # 跑面板测试
```

`script/build-zed` 存在的原因（裸 `cargo build` 在 NixOS 上会失败）：

1. 系统里没有 `x11.pc`，`PKG_CONFIG_PATH` 为空 → 脚本进入 `nix-shell` 提供 X11/Wayland 依赖；
2. 宿主 Rust 版本不符 → 脚本挑出 `/nix/store/*-rustc-1.98.1`；
3. `webrtc-sys` 构建时会从 GitHub 下载 131 MB 预编译包 → 脚本优先解压仓库内的
   `webrtc-linux-x64-release.zip` 并通过 `LK_CUSTOM_WEBRTC` 指过去（离线可用）。

### 其它发行版

```bash
./script/linux          # 上游维护的系统依赖安装脚本（apt/dnf/pacman…）
cargo build -p zed     # 首次约 30–60 分钟；webrtc-sys 会自动下载预编译包
cargo run -p zed
```

首次构建需要约 15–25 GB 磁盘；可用 `CARGO_PROFILE_DEV_DEBUG=0` 显著压小体积。

### 打包成 Flatpak 安装到本地

```bash
script/build-flatpak            # 需要时就先 script/build-zed --release，再打包并安装
script/build-flatpak --skip-build   # 用现有 target/release/zed
script/build-flatpak --bundle       # 额外导出 target/release/io.github.qipn.ZedFileManager.flatpak
flatpak run io.github.qipn.ZedFileManager
```

装好后也能从应用网格"Zed File Manager"启动（`.desktop` + 图标 + metainfo 都装进去了）。

清单在 `flatpak/io.github.qipn.ZedFileManager.yml`，要点：

- **不在沙箱里编译**：`zed` 的直接依赖（glib/gio、freetype、alsa、xcb、xkbcommon、libstdc++）
  都在 `org.freedesktop.Platform` 里，其余（wayland/vulkan/libva…）是运行时 dlopen，
  所以直接打包已编译的二进制，构建只要几秒；代价是不用 SDK 里的 Rust 工具链
- **NixOS 特有**：宿主机构建的二进制，ELF 解释器与 RUNPATH 都指向 `/nix/store`，
  沙箱默认看不到 → 会报 `bwrap: execvp zed: No such file or directory`。
  清单里加了 `--filesystem=/nix/store:ro` 让它能解析（沙箱因此变宽，这是不在沙箱内
  重编译的代价）
- app id 用 `io.github.qipn.ZedFileManager`（与上游 Flathub 的 `dev.zed.Zed` 区分开）
- desktop 的 `Exec` 用 `zed --foreground %U`；编辑器二进制**不认 `--new`**（那是 CLI 的参数），
  所以没有单独的"New Workspace"动作
- 依赖 `org.freedesktop.Sdk//24.08` 与 `org.freedesktop.Platform//24.08`：
  `flatpak install flathub org.freedesktop.Sdk//24.08`
- `flatpak/build/`、`flatpak/build-input/`、`flatpak/.builder/` 已在 `.gitignore`

快速自检（不编译 Zed，验证清单/运行时/安装链路）：

```bash
script/build-flatpak --binary /run/current-system/sw/bin/true
flatpak run io.github.qipn.ZedFileManager   # 桩程序会报 “unknown program 'zed'”，说明能执行
flatpak uninstall -y io.github.qipn.ZedFileManager
```

---

## 4. 测试

```bash
script/build-zed --test -p file_manager    # 或：cargo test -p file_manager
```

当前 **40 个用例**，覆盖：

- 渲染冒烟（整面板 / 网格 + 详情面板 / 面板在 Dock 内）
- 文本输入（搜索框、Dock 内输入、`key_char` 缺失兜底、路径编辑的 Enter/Esc）
- 侧栏树（跟随当前目录、离开分支折叠、树根粘滞、`/bin` 不丢树）
- 图标解析（`.rs`/`.c`/`.h`/未知后缀/目录/展开目录）
- 状态持久化（serde 往返、缺字段默认值、越界夹紧、kvp 读写）
- 纯逻辑单测：xdg 目录、GTK 书签、Places 分组

`crates/file_manager` 不依赖 `livekit`/`webrtc`，所以这个测试命令很轻，适合放进 CI。

---

## 5. 代码结构

```
crates/file_manager/
├── file_manager.rs          # crate 根：docs、actions!、init()、reveal_in_file_manager()
├── model/                   # 数据与规则层（不依赖 ui/workspace 实体）
│   ├── entries.rs           #   DirEntry / SortMode / read_entries / sort_entries
│   ├── search.rs            #   递归搜索
│   ├── tree.rs              #   文件夹树节点 + 懒加载 + 可见行/祖先链
│   ├── clipboard.rs         #   剪贴板与复制/移动助手
│   ├── conflict.rs          #   重名冲突策略（替换/跳过/都保留）
│   ├── undo.rs              #   双栈撤销/重做
│   ├── xdg.rs               #   home、user-dirs.dirs
│   └── bookmarks.rs         #   GTK bookmarks 文件读写
├── ui/                      # gpui 层
│   ├── panel.rs             #   FileManagerPanel 实体 + Panel/Focusable/Render impl
│   ├── panel/               #   按职责拆分的面板行为
│   │   ├── navigation.rs    #     目录导航、历史栈、打开条目、作为工作区打开
│   │   ├── location.rs      #     位置槽状态机 + 面包屑目标/菜单
│   │   ├── selection.rs     #     选择/键盘导航/按键转发
│   │   ├── file_ops.rs      #     新建/删除/剪贴板
│   │   ├── rename.rs        #     内联重命名
│   │   ├── transfer.rs      #     拖放落点、冲突解决、传输
│   │   ├── search.rs        #     搜索模式调度
│   │   ├── sidebar_state.rs #     Places/树：粘滞根、reveal_in_tree、滚动定位
│   │   ├── view_state.rs    #     视图模式/图标尺寸/就地展开
│   │   ├── persistence.rs   #     UI 状态持久化（kvp）
│   │   ├── sort.rs / history.rs / bookmarks.rs / layout.rs / menus.rs
│   ├── render/              # 布局装配（不持有状态）
│   │   ├── header.rs        #     单行 header bar
│   │   ├── location.rs      #     面包屑 / 地址编辑 / 搜索框
│   │   ├── body.rs / status_bar.rs / conflict_banner.rs
│   ├── views/               # 详情列表、图标网格、图标解析、格式化
│   ├── search_field/        # 自实现的单行输入（EntityInputHandler，含 IME）
│   ├── sidebar.rs / places.rs / details_pane.rs / drag.rs / ox_theme.rs
└── tests.rs                 # 渲染/输入/树的集成测试
```

约束：

- **依赖方向单向**：`ui` → `model`，`model` 不反向依赖
- **不能依赖 `editor`**：`editor` 依赖本 crate 做 reveal，反向会成环
- 单文件目标 ≤500 行（当前 `ui/panel.rs` 519、`ui/panel/menus.rs` 503 略超，
  `tests.rs` 832 属测试集合）

### 集成点（相对上游改动的上游文件）

| 文件 | 改动 |
|---|---|
| `Cargo.toml` / `Cargo.lock` | 注册 workspace crate 与依赖 |
| `crates/zed/Cargo.toml`、`crates/zed/src/zed.rs` | 启动时注册面板（`initialize_panels`）|
| `crates/editor/src/editor.rs`、`crates/project_panel`、`crates/outline_panel` | “Reveal in File Manager” 优先交给本面板 |
| `assets/keymaps/default-linux.json` | 新增 `FileManagerPanel` 上下文键位块 |
| `script/build-zed` | NixOS 构建脚本 |
| `.github/workflows/file-manager.yml` | CI |

---

## 6. 已知限制 / 未做

- header bar 三处 tooltip 的快捷键未绑定（见 §2）
- 列宽不可拖拽调整、列选择与列宽未持久化
- 大目录粘贴无进度条与取消（`ProgressInfo` 未实现）
- 冲突对话框只有内联提示条，缺「应用于全部」与“建议名可编辑”
- 搜索只有名称匹配：无范围/MIME/日期过滤，不搜文件内容
- 批量重命名、空格键快速预览、路径输入自动补全未做
- 网格视图仍会渲染就地展开目录的子项（与 Nautilus 不一致）
- 剩余空间/驱动器容量条未做（需要 `statvfs`）
- 左侧树的行没有右键菜单；Places 行的菜单也没有「作为工作区打开」

---

## 7. 与上游的关系

```bash
git remote -v          # origin 指向 zed-industries/zed，mine 指向自己的 fork
git fetch origin
git rebase origin/main # 我们的提交集中在 file_manager 与少量集成点，冲突通常很少
```

本仓库的提交只触碰上表那些文件；`crates/file_manager` 全部是新增文件，
因此合并上游改动时一般不需要处理该 crate 的冲突。

## 8. 许可

- 上游 Zed：GPL-3.0 / Apache-2.0 双许可（见 `LICENSE-GPL`、`LICENSE-APACHE`）
- `crates/file_manager`：`GPL-3.0-or-later`（见其 `Cargo.toml` 与 `LICENSE-GPL`）

---

## 附录：推送到自己的 GitHub 并启用自动构建

仓库已带 `.github/workflows/file-manager.yml`：

- **每次 push / PR**（且改动落在 `crates/file_manager` 或其依赖）：
  `./script/linux` 装依赖 → 按 `rust-toolchain.toml` 装 1.98.1 → `cargo test -p file_manager`
- **手动触发**（Actions → *File manager* → Run workflow，勾选 `build_app`）：
  `cargo build -p zed` 并上传 `target/debug/zed` 工件（约 40 分钟）

推送步骤：

```bash
# 方式 A：gh CLI（会创建仓库并推送）
gh auth login
gh repo create <用户名>/zed-filemanager --private
git push -u mine main          # mine = git@github.com:<用户名>/zed-filemanager.git

# 方式 B：先在网页建一个空仓库（不要勾 README/gitignore/license），再
git push -u mine main
```

注意事项：

- 首次推送会上传完整历史（`.git` 约 588 MB），之后增量；`target/` 已被忽略
- 上游自带几十个 workflow 会被 push 触发且大概率失败，建议只留本仓库的这一个：

```bash
for wf in $(gh workflow list --all --json name --jq '.[].name' | grep -v '^File manager$'); do
  gh workflow disable "$wf"
done
```

- `script/build-zed` 是给 NixOS 本地用的，**不要**在 CI 里调用（CI 用 apt 依赖 + 裸 cargo）
