# Zed File Manager（Zed fork）

[![File manager](https://github.com/Qipn/zed-filemanager/actions/workflows/file-manager.yml/badge.svg)](https://github.com/Qipn/zed-filemanager/actions/workflows/file-manager.yml)

本仓库是 [zed-industries/zed](https://github.com/zed-industries/zed) 的 fork，在内置面板体系里
新增了一个 **GNOME Files 风格的文件管理器 Dock 面板**，代码在 `crates/file_manager`。
除该 crate 与少量集成点外，其余与上游完全一致。

设计取向：

- **外壳与交互**对齐 **GNOME Files（Nautilus）**：单行 header bar、面包屑/路径编辑/搜索
  共用一个位置槽、侧栏 Places 分组、行内联重命名
- **内容区尺寸与列**参考 **OpenXplorer** 的 UI 规格（`openxplorer/native/docs/ui-spec.md`）
- **配色全部取自 Zed 主题**，不引入第三方色板，与编辑器其它面板一致

## 快速开始（NixOS）

```bash
script/build-zed run                        # 构建并启动
script/build-zed --test -p file_manager     # 跑面板测试（48 个用例）
script/build-flatpak                        # 打包成 Flatpak 并安装到本地
```

---

## 1. 功能

### 浏览与导航

- 目录列表（`uniform_list` 虚拟化）与图标网格两种视图（`Ctrl+Shift+I` 切换）
- 详情视图四列：Name | Date modified | Type | Size，表头点击排序（升/降序箭头）
- **列可拖拽调整宽度**：`Date modified` / `Type` / `Size` 每列右侧 8px 抓取区，
  宽度与显隐（View 菜单里的 `X column ✓`）都会持久化；`Name` 是唯一的弹性列
- **详情视图内就地展开目录**：目录行前 16px 箭头，子项按层级缩进，懒加载；
  **图标网格不显示就地展开的子项**（对齐 Nautilus 的图标视图）
- 历史栈：后退 / 前进 / 上级 / 刷新
- **位置槽三态共用**：面包屑 / 可编辑路径 / 搜索
  - 根段是地址栏前的文件夹图标（点击回 `/`），祖先段左键跳转、右键弹出「跳转到此目录 +
    同级目录」菜单，`…` 可点开被省略的祖先
  - 最后一段（当前目录）点击 = 进入编辑（对齐 Nautilus 的 `toolbar.edit-location`）
  - 编辑态支持 `~`、相对路径、IME、选区、`Ctrl+A/C/X/V`；`Enter` 确认 / `Esc` 取消
  - **`Tab` 补全路径**：唯一命中目录会补上 `/`，多个命中取公共前缀
- **递归搜索**（`model/search.rs`）：深度上限 8、结果上限 2000，按名称不区分大小写；
  结果扁平列出，状态栏显示 `N results`（被截断时 `N+`），键入去抖 120ms
- **搜索过滤器**（搜索框右侧齿轮）：范围（递归 / 仅当前层）、类型（Any / Images /
  Documents / Code）、修改时间（7 / 30 天）、**是否搜文件内容**（仅在名称未命中时读文件，
  超过 4 MiB 跳过）。文件夹永远不被类型与时间过滤——它是进入内部文件的路径
- 隐藏文件开关、按名称/修改时间/大小排序
- 右侧详情面板：预览、名称、类型、Open 按钮、大小/修改时间/位置
- **`Space` 快速预览**：打开/关闭详情面板预览；图片直接渲染，文本文件异步读前 2000 字符
- **状态栏容量条**：当前目录所在文件系统的 `剩余 / 总容量`（`statvfs`，仅本地目录）

### 文件操作

- 新建文件夹 / 新建文件（`New ▾` 菜单：Folder / Text document / File / Markdown / CSV / JSON / HTML）
- **内联重命名**：新建后立即进入编辑，只选中主名（文件不含扩展名）；校验空名、`/`、
  首尾空格、同名冲突
- **批量重命名**：多选后右键 `Batch rename…`，模板里的 `#` 依次替换为 `1..N`；
  模板没写扩展名时保留各自原扩展名；目标已存在则报错中止而不是覆盖；整批记成一次可撤销的变更
- 复制 / 剪切 / 粘贴（面板内剪贴板，粘贴到当前目录）
- **硬链接复制**：右键 `Copy as Hard Link` 把选区放进硬链接模式的剪贴板，
  下一次 `Paste` 在目标目录建立**硬链接**而不是复制数据（状态栏显示 `(linked)`）。
  - 文件夹会自动递归：先按原层级建目录（空目录也保留），再把每个文件链接过去
    （即 `cp -al` 的语义）—— `link(2)` 对目录返回 `EPERM`，所以只能这样
  - 硬链接要求源和目标是**同一文件系统**，跨盘会报错（不会静默降级成复制）
  - 链接后两边共享 inode：改其中一个，另一个也变；删掉任一个不丢数据
  - 撤销（`Ctrl+Z`）是把新建的链接移进回收站，源文件不受影响
- **拖放**：拖动已选中行时带上整个选区；支持从外部（文件管理器/桌面）拖入；
  落点亮色用 Zed 的 `drop_target_background`。
  **拖到原地是空操作**：把条目拖到它自己的行上、或者拖到它所在目录的空白处，
  不会改名成 `A (1)`（`Replace` 分支以前会先删掉再改名，有丢数据的风险），
  只有 Ctrl 拖拽（复制）才会在原地生成 `A (1)`
- 删除：`Move to Trash` 走 `Fs::trash`（freedesktop 回收站，可撤销）、
  `Delete Permanently` 直接删除；两者都有确认弹窗
- **传输进度与取消**：粘贴/拖入时头部下方显示 `Copying N of M — name` + 进度条 +
  Cancel；取消在条目边界生效，已完成的部分照常进撤销历史
- **重名冲突逐条处理（弹窗）**：冲突时弹出一张模态卡片（Zed 的 `elevation_3` 卡片 +
  绝对定位遮罩），显示 `“X” 已存在 (1 of N)`，建议名可编辑，
  回答为 `Use name`（改名，不动已有文件）/ `Replace` / `Skip`，并可 **Apply to all**；
  输入了一个已存在的名字也不会静默覆盖。`Enter` = 用建议名，`Esc` = 取消整个传输；
  点遮罩**不会**取消（避免误点打断传输）
- **撤销 / 重做**（`Ctrl+Z` / `Ctrl+Shift+Z`）：新建、重命名（含批量）、复制、移动、回收站均可回滚

### 侧栏

- **Places**（对齐 `nautilus-sidebar.c::update_places`）：Home → 快速访问（读
  `~/.config/user-dirs.dirs`，支持本地化名）→ 回收站 → 项目根 → 卷 → 网络（读 `/proc/mounts`）
  → 书签；分组之间是 1px 分隔线，重复路径去重
- **书签**：与 GTK 共用 `$XDG_CONFIG_HOME/gtk-3.0/bookmarks`，`Ctrl+D` 收藏当前目录，
  行内 `×` 取消，右键 Open / Add·Remove / Copy Path
- **文件夹树**：懒加载；**点击行只导航，展开只由 `>` 触发**；
  **树根粘滞**（在 home 内以 home 为根，之外以 `/` 为根，不因导航而重建）；
  **严格镜像当前路径**——展开到当前目录、把当前行滚进可见区、离开的分支跟着折叠
- **右键菜单**：文件夹树的行与 Places 的行都有（Open / Open in This Window /
  Open in New Window / 书签 / Copy Path；Places 里回收站不给工作区入口）
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
侧栏开关与宽度、Places/树分栏比例、隐藏文件开关、排序方式、列表/网格、图标尺寸、
详情面板开关、**列宽与列显隐**。
写入去抖 250ms，读取时对越界值做夹紧；**浏览的目录不持久化**（始终跟随当前项目）。

---

## 2. 交互速查

| 快捷键 | 作用 |
|---|---|
| `Ctrl+F` | 打开/关闭搜索 |
| `Ctrl+L` | 编辑路径 |
| `Tab` | 路径编辑时补全最后一段 |
| `Space` | 预览选中项（再按一次关闭）|
| `Enter` | 确认当前内联输入（路径 / 重命名 / 批量重命名）|
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
拖动分栏线调整宽度/比例（双击复位）、拖动列头右侧改列宽。

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

release 配置默认是 `lto = "thin"` + `codegen-units = 1`（编译很慢）。只为本机打包时可以用：

```bash
script/build-zed --release -- -j 4 \
  --config 'profile.release.lto=false' \
  --config 'profile.release.codegen-units=16' \
  --config 'profile.release.debug=0'
```

### 原生安装（日常使用推荐）

装成普通用户应用，不走 Flatpak（沙箱里没有宿主工具链，终端/agent/构建都别扭）：

```bash
script/build-zed --release              # 编 editor
script/build-zed --release -- -p cli    # 单实例网关（只需编一次）
script/install-native                   # 硬链接安装
script/install-native --uninstall
```

装出来的布局：

| 路径 | 是什么 |
|---|---|
| `~/.local/bin/zeditor` | **CLI**（硬链接 `target/release/cli`），单实例网关 |
| `~/.local/lib/zed/zed-editor` | **editor 本体**（硬链接 `target/release/zed`）|
| `~/.local/share/applications/dev.zed.Zed-Dev.desktop` | 桌面条目（名字 = 窗口 app_id）|
| `~/.local/share/icons/hicolor/512x512/apps/io.github.qipn.ZedFileManager.png` | 图标 |
| `~/.local/share/metainfo/io.github.qipn.ZedFileManager.metainfo.xml` | AppStream |

#### 为什么是两个文件（CLI + editor）

单实例是 **CLI** 实现的（`crates/cli/src/main.rs` 的 `launch()`）：它连
`~/.local/share/zed/zed-<channel>.sock`，连上就把请求（`zed-cli://…`）发过去，
连不上才 fork 起一个 editor。只装 editor 的话，从菜单栏每点一次就新起一个进程——
这就是“点固定图标反而又冒出一个图标”的第一个原因。

CLI 找 editor 的顺序是 `../libexec/zed-editor`、`../lib/zed/zed-editor`、`./zed`，
所以 editor 放 `~/.local/lib/zed/` 正好被找到，**且不占用 `zed` 这个名字**。

#### 为什么 desktop 文件叫 `dev.zed.Zed-Dev.desktop`

Wayland 下 GNOME 用窗口的 `app_id` 去匹配**同名**的 `.desktop` 文件；X11 下看
`StartupWMClass`。`app_id` 来自 `ReleaseChannel::app_id()`，我们的版本串是
`1.28.0+dev` → `dev.zed.Zed-Dev`。名字对不上，面板就会把窗口当成另一个应用，
固定的图标旁边再生一个——这是第二个原因。所以脚本生成的桌面条目就叫这个名字，
并写上 `StartupWMClass=dev.zed.Zed-Dev`。

nixpkgs 的 zed 没这问题，因为它装的就是 `dev.zed.Zed.desktop`（stable 的 id）。

#### 硬链接的取舍

零字节开销（`stat -c %i` 看 inode 相同，`ls -l` 看链接数 +1）。代价：

- 目标目录与 `target/` 必须同一文件系统，否则脚本自动退化成拷贝
- `cargo clean` / 删 target 后链接悬空，重跑一次安装即可
- 重新编译会换 inode，**要重跑 `script/install-native`** 才会用上新产物

#### Wayland 库路径

`gpui` 用 `dlopen("libwayland-client.so.0")` 连 Wayland，而进程跑在 nix 的 `ld.so`
下，看不到运行时那份，会直接 panic：

```
failed to connect to Wayland compositor: The wayland library could not be loaded
```

所以脚本把 nix 的 wayland 目录写进桌面条目的 `Exec=env LD_LIBRARY_PATH=…`（只影响
这一个程序，不污染会话）。它**只在当前没有实例、需要新起 editor 时**才用得到；
已经有实例在跑时 CLI 只发一个 datagram，任何 shell 直接 `zeditor .` 都能用。

#### 一个曾经的坑（E2BIG）

早期版本用“sh 启动器 + 真二进制”两个文件，并支持 `--link`。`--link` 装完后再跑一次
非 link 的安装，`install -Dm755 <binary> <name>-bin` 会**透过符号链接**写回源文件，
把 356 MB 的 `target/release/zed` 覆盖成 509 字节的启动器脚本；之后两个启动器互相
`exec`，每轮又往 `LD_LIBRARY_PATH` 拼一次，环境变量越滚越大，最终 `execve` 返回
**E2BIG（参数列表过长）**。

现在没这个问题了：只有硬链接，没有启动器；安装前先 `rm -f` 目标、源与目标同路径
直接拒绝；`--uninstall` 只删 `~/.local` 下的文件，**永远不碰** `/run/current-system`。

> **不要叫 `zed`**：NixOS 系统 profile 里的 `zed` 是 **ZFS Event Daemon**
> （`zfs-user` 的 `/sbin/zed -F`），遮蔽了会弄丢那个工具。编辑器命令用 `zeditor`。

万一还是把 `target/release/zed` 弄坏了：从之前的 flatpak bundle 里能捞回来（那版还没
拆 launcher，二进制就叫 `zed`，已 strip 但能直接用）：

```bash
flatpak install --user -y target/release/io.github.qipn.ZedFileManager.flatpak
SRC=~/.local/share/flatpak/app/io.github.qipn.ZedFileManager/current/active/files/bin/zed
rm -f target/release/zed && cp --reflink=auto "$SRC" target/release/zed && chmod 755 target/release/zed
flatpak uninstall --user -y io.github.qipn.ZedFileManager
```

> **装完要重启一次**：正在跑的实例如果是旧布局（直接 exec editor）启动的，它没有创建
> `zed-dev.sock`，所以第一次点图标仍会新开一个实例。关掉后从图标重新启动一次，之后
> 就都是“在当前实例开窗口”了。

### 打包成 Flatpak 安装到本地

```bash
script/build-flatpak                # 需要时先 --release，再打包并安装
script/build-flatpak --skip-build   # 用现有 target/release/zed
script/build-flatpak --bundle       # 额外导出 target/release/io.github.qipn.ZedFileManager.flatpak
flatpak run io.github.qipn.ZedFileManager
```

装好后也能从应用网格 "Zed File Manager" 启动（`.desktop` + 图标 + metainfo 都装进去了）。

#### 启动 / 升级 / 卸载

`script/build-flatpak` 内部是 `flatpak-builder --user --install`，直接装进
`~/.local/share/flatpak/app/io.github.qipn.ZedFileManager`，**不需要**再手动
`flatpak install`。装完约 373 MB（二进制会被 strip，debug 符号单独放 `Debug` 运行时）。

```bash
flatpak run io.github.qipn.ZedFileManager                # 启动
flatpak run io.github.qipn.ZedFileManager ~/some/dir     # 以指定目录为工作区启动

# 升级：重编 + 重新打包（同一个 app id 原地覆盖，不会并存旧版本）
script/build-zed --release
script/build-flatpak --skip-build

# 卸载（加 --delete-data 连同 ~/.var/app 下的配置一起删）
flatpak uninstall --user io.github.qipn.ZedFileManager
```

`--bundle` 导出的 `target/release/io.github.qipn.ZedFileManager.flatpak`（约 79 MB）
可以在同一台机器上重新装、或拷给**另一台 NixOS 机器**：

```bash
flatpak install --user ./io.github.qipn.ZedFileManager.flatpak
```

> ⚠️ 这个 bundle **不能**拷到非 NixOS 发行版。里面的 `zed` 是宿主机构建的，ELF
> 解释器写死成 `/nix/store/…-glibc-2.42-103/ld-linux-x86-64.so.2`，清单里又用
> `--filesystem=/nix/store:ro` 挂进沙箱；目标机没有同一条 store 路径就起不来。
> 换发行版请在那台机器上 `script/build-zed --release` 后重新打包。

`flatpak list --app --columns=application,version,size` 里版本显示为 `dev`
（本项目没打版本号，`metainfo` 也没写 release 条目）。

#### 把现有 Zed 的数据迁过来（硬链接，几乎不占空间）

Flatpak 版的数据在 `~/.var/app/<id>/{data,config}/zed`，与 `~/.local/share/zed`、
`~/.config/zed` 完全隔离。`script/link-zed-data` 用硬链接把两边接起来：

```bash
script/link-zed-data                        # 先看计划（dry-run，默认不改任何东西）
script/link-zed-data --yes                  # 执行（要求两个 Zed 都退出）
script/link-zed-data --yes --while-running  # 不想关编辑器时（db 只能是快照）
```

策略：

| 内容 | 方式 | 原因 |
|---|---|---|
| `extensions` `languages` `node` `external_agents` `prettier` | **硬链接**（同一 inode） | 下载一次就只读，共约 1 GB，不占额外空间 |
| `db/0-stable` → `db/0-dev` | 真复制 | SQLite 被两个**不同版本**共用 inode 时，schema 迁移会互相写坏 |
| `threads/threads.db` | 真复制 | 同上（138 MB） |
| `settings.json` / `keymap.json` / `tasks.json` | 真复制 | Zed 用「写临时文件 + rename」，rename 会断开硬链接 |
| `themes/` 等只读目录 | 硬链接 | 不会被就地改写 |
| `logs` | 不迁移 | 没有价值 |

细节：

- 目标侧已有的文件默认**不覆盖**（`cp -n`），要覆盖加 `--force`（先删再链）
- 被覆盖前一律先备份成 `*.bak-<时间戳>`（`db/0-dev`、`threads`、`prompts`、`settings.json` 都备份）
- db 目录名带 channel 后缀：原生稳定版是 `0-stable`，而本 fork 的版本串是
  `1.28.0+dev`、读的是 `0-dev`，脚本会自动映射（`--db-from` / `--db-to` 可改）
- 两边不在同一文件系统时自动退化成真复制（会 print 一条 warn）

实测（52k 文件、1.2 GB，**2.5 秒**完成）：`extensions` 8408/8419、`languages` 25453/30425、
`node` 4946/9754 个文件与原生共享 inode；剩余差异是 Flatpak 侧原本就有同名文件，被 `-n` 跳过了。

已知副作用：

- `node/` 会出现两份（本 fork 自己下过一个），`du` 从 114M 涨到 218M，可手动删掉不用的那份
- 迁过来的旧 agent 会话在**回放工具调用**时会刷 `missing field 'path'` / `expected ':'` 之类
  的 ERROR（`crates/agent/src/thread.rs` 的 `tool.replay(...).log_err()`），**非致命**，
  是 1.26 与 1.28 的工具参数格式差异；介意就删掉 `threads/` 让它重建

清单在 `flatpak/io.github.qipn.ZedFileManager.yml`，要点：

- **不在沙箱里编译**：`zed` 的直接依赖（glib/gio、freetype、alsa、xcb、xkbcommon、libstdc++）
  都在 `org.freedesktop.Platform` 里，其余（wayland/vulkan/libva…）是运行时 dlopen，
  所以直接打包已编译的二进制，构建只要几秒；代价是不用 SDK 里的 Rust 工具链
- **NixOS 特有**：宿主机构建的二进制，ELF 解释器与 RUNPATH 都指向 `/nix/store`，
  沙箱默认看不到 → 会报 `bwrap: execvp zed: No such file or directory`。
  清单里挂的是整个 `--filesystem=/nix:rw`（不是 `store:ro`）：除了让 loader 找到二进制，
  还要让沙箱里的 nix 读 profile/channel 并连上宿主 daemon（见下一节）
- **显卡驱动也要挂**：NixOS 的 GL/Vulkan 驱动在 `/run/opengl-driver`，不在 `/usr/lib`。
  不挂它，wgpu 一个 HAL 后端都建不起来，报
  `Failed to create surface for any enabled backend: {}`。清单里加了
  `--filesystem=/run/opengl-driver:ro` 和 `LD_LIBRARY_PATH=/run/opengl-driver/lib`
  （`libvulkan.so.1` 是 dlopen 的）
- **原生 Wayland**：gpui 的 `wayland-client` 是 dlopen 的，而进程跑的是 nix 的 `ld.so`，
  它搜不到运行时里的 `libwayland-client`；而 `LD_LIBRARY_PATH` 的优先级高于 RUNPATH，
  所以**只能放 nix 那一份**——运行时的 1.23 比 nix mesa 26.x 需要的 1.25 老，缺
  `wl_fixes_interface`，会让 mesa 的 EGL/Vulkan 驱动一并加载失败
  （`Client extensions: []` / `ERROR_INCOMPATIBLE_DRIVER`）。而那个 store 路径是打包机相关的，
  不能写进仓库里的静态清单，所以 `script/build-flatpak` 会解析出来（走 glvnd 的
  `50_mesa.json` → mesa 的 RUNPATH，拿到与二进制实际链接一致的那份），生成一个 launcher：
  - `/app/bin/zed-bin` —— 真正的二进制
  - `/app/bin/zed` —— 生成的 sh shim：把 nix 的 wayland 放进 `LD_LIBRARY_PATH` 后 `exec zed-bin`

  打包机换了 nix store 路径时要重新 `script/build-flatpak`（可用 `ZED_WAYLAND_LIB=...` 覆盖）。
  副作用：`wgpu` 因此选到了 **Vulkan（RADV）** 而不是 GL，比之前的 GL 后端快不少
- app id 用 `io.github.qipn.ZedFileManager`（与上游 Flathub 的 `dev.zed.Zed` 区分开）
- desktop 的 `Exec` 用 `zed %U`。**不要写 `--foreground`**：那个参数属于 `crates/cli`
  的 CLI 包装器，我们打包的是 `crates/zed` 的 app 二进制，会报
  `unexpected argument '--foreground'`
- 依赖 `org.freedesktop.Sdk//24.08` 与 `org.freedesktop.Platform//24.08`：
  `flatpak install flathub org.freedesktop.Sdk//24.08`
- `flatpak/build/`、`flatpak/build-input/`、`flatpak/.builder/` 已在 `.gitignore`

快速自检（不编译 Zed，验证清单/运行时/安装链路）：

```bash
script/build-flatpak --binary /run/current-system/sw/bin/true
flatpak run io.github.qipn.ZedFileManager   # 桩程序会报 "unknown program 'zed'"，说明能执行
flatpak uninstall -y io.github.qipn.ZedFileManager
```

### 关于「让终端跑在宿主」的尝试（已撤销）

曾经做过一版：`crates/terminal/src/terminal.rs` 检测到 `/.flatpak-info` 就把启动命令包成
`flatpak-spawn --host --directory=<cwd> <shell>`，配合 `--talk-name=org.freedesktop.Flatpak`、
`/nix:rw`、`/run/current-system:ro` 和 launcher 里前置系统 profile 的 `PATH`，让 Flatpak 里的
终端和 agent 能直接跑 `script/build-zed`。

**这套已经撤掉了**（提交 `a016904a65` 被回滚）：

- 它把沙箱隔离基本废掉（`--talk-name=org.freedesktop.Flatpak` + `/nix:rw`），只适合临时开发，
  不适合当发布包
- 日常开发有更简单的路子：`script/install-native` 直接装到 `~/.local`，没有沙箱这一层
- 撤销后 Flatpak 清单回到最小可用权限集（`/nix/store:ro`、`/run/opengl-driver:ro` + Wayland/
  X11/DRM），终端就在沙箱里跑

如果哪天真需要，思路仍在：`flatpak-spawn --host` 走 session D-Bus portal（VS Code /
GNOME Builder 的做法，Flatpak 1.15+ 转发 stdin 所以 pty 可用），但必须显式传一个宿主存在的
shell（`flatpak-spawn` 不允许省略命令）。

---

## 4. 测试

```bash
script/build-zed --test -p file_manager    # 或：cargo test -p file_manager
```

当前 **48 个用例**，覆盖：

- 渲染冒烟（整面板 / 网格 + 详情面板 / 面板真的在 Dock 内）
- 文本输入（搜索框、Dock 内输入、`key_char` 缺失兜底、路径编辑的 Enter/Esc）
- 侧栏树（跟随当前目录、离开分支折叠、树根粘滞、`/bin` 不丢树、滚动定位）
- 视图（网格不渲染就地展开的子项、列宽拖拽与显隐的持久化）
- 搜索（范围/类型/内容过滤）、路径 `Tab` 补全、批量重命名、冲突自定义名
- 图标解析（`.rs`/`.c`/`.h`/未知后缀/目录/展开目录）
- 状态持久化（serde 往返、缺字段默认值、越界夹紧、kvp 读写）、`statvfs` 容量
- 纯逻辑单测：xdg 目录、GTK 书签、Places 分组

`crates/file_manager` 不依赖 `livekit`/`webrtc`，所以这个测试命令很轻，适合放进 CI。

---

## 5. 代码结构

```
crates/file_manager/
├── file_manager.rs          # crate 根：docs、actions!、init()、reveal_in_file_manager()
├── model/                   # 数据与规则层（不依赖 ui/workspace 实体）
│   ├── entries.rs           #   DirEntry / SortMode / read_entries / sort_entries
│   ├── search.rs            #   递归搜索 + SearchOptions（范围/类型/时间/内容）
│   ├── tree.rs              #   文件夹树节点 + 懒加载 + 可见行/祖先链
│   ├── clipboard.rs         #   剪贴板与复制/移动助手
│   ├── conflict.rs          #   重名冲突策略与 apply_transfer/unique_target
│   ├── undo.rs              #   双栈撤销/重做（含 Batched）
│   ├── fs_space.rs          #   statvfs 容量
│   ├── xdg.rs               #   home、user-dirs.dirs
│   └── bookmarks.rs         #   GTK bookmarks 文件读写
├── ui/                      # gpui 层
│   ├── panel.rs             #   FileManagerPanel 实体 + Panel/Focusable/Render impl
│   ├── panel/               #   按职责拆分的面板行为
│   │   ├── navigation.rs    #     目录导航、历史栈、打开条目、作为工作区打开
│   │   ├── location.rs      #     位置槽状态机、面包屑菜单、Tab 补全
│   │   ├── selection.rs     #     选择/键盘导航/按键转发/空格预览入口
│   │   ├── file_ops.rs      #     新建/删除/剪贴板
│   │   ├── rename.rs        #     内联重命名
│   │   ├── batch_rename.rs  #     批量重命名（模板 #）
│   │   ├── transfer.rs      #     拖放落点、冲突逐条解决、传输进度与取消
│   │   ├── search.rs        #     搜索模式调度
│   │   ├── preview.rs       #     空格预览（文本片段/图片判定）
│   │   ├── sidebar_state.rs #     Places/树：粘滞根、reveal_in_tree、滚动定位
│   │   ├── view_state.rs    #     视图模式/图标尺寸/就地展开
│   │   ├── columns.rs       #     列宽拖拽计算与列显隐
│   │   ├── persistence.rs   #     UI 状态持久化（kvp）
│   │   ├── sort.rs / history.rs / bookmarks.rs / layout.rs / menus.rs
│   ├── render/              # 布局装配（不持有状态）
│   │   ├── header.rs        #     单行 header bar
│   │   ├── location.rs      #     面包屑 / 地址编辑 / 搜索框 + 过滤菜单
│   │   ├── body.rs / status_bar.rs
│   │   ├── conflict_banner.rs / transfer_progress.rs / batch_rename.rs
│   ├── views/               # 详情列表、图标网格、图标解析、格式化
│   ├── search_field/        # 自实现的单行输入（EntityInputHandler，含 IME）
│   ├── sidebar.rs / places.rs / details_pane.rs / drag.rs / ox_theme.rs
└── tests.rs                 # 渲染/输入/树的集成测试
```

约束：

- **依赖方向单向**：`ui` → `model`，`model` 不反向依赖
- **不能依赖 `editor`**：`editor` 依赖本 crate 做 reveal，反向会成环
- 单文件目标 ≤500 行。当前超出的是 `ui/panel/menus.rs`（789）、`ui/panel.rs`（599）
  与 `tests.rs`（1199，测试集合），属于待拆的技术债

### 集成点（相对上游改动的上游文件）

| 文件 | 改动 |
|---|---|
| `Cargo.toml` / `Cargo.lock` | 注册 workspace crate 与依赖 |
| `crates/zed/Cargo.toml`、`crates/zed/src/zed.rs` | 启动时注册面板（`initialize_panels`）|
| `crates/editor/src/editor.rs`、`crates/project_panel`、`crates/outline_panel` | “Reveal in File Manager” 优先交给本面板 |
| `assets/keymaps/default-linux.json` | 新增 `FileManagerPanel` 上下文键位块 |
| `script/build-zed`、`script/build-flatpak`、`script/link-zed-data` | NixOS 构建、本地 Flatpak 打包、数据迁移 |
| `flatpak/` | Flatpak 清单 + desktop + metainfo |
| `.github/workflows/file-manager.yml` | CI |

---

## 6. 已知限制 / 未做

- header bar 三处 tooltip 的快捷键未绑定（见 §2）
- 传输进度按顶层条目计数，目录内部不细分；取消在条目边界生效
- 冲突提示是面板内联提示条（非遮罩层对话框），也还没显示源/目标的大小与时间对比
- 搜索的类型是扩展名分组，没有精确 MIME 选择；不支持正则、按大小过滤
- 批量重命名只有模板模式（`#`），没有查找替换，也没有创建时间/相机型号等其余模板标签
- 空格预览只覆盖图片与纯文本，Markdown 不渲染、音视频不播放
- 列头没有 hover 的 1px 分隔线；没有全盘搜索、真实缩略图、紧凑视图、选择复选框
- `ui/panel.rs` / `ui/panel/menus.rs` 超过单文件 500 行约定，待拆

---

## 7. 与上游的关系

```bash
git remote -v          # origin 指向 zed-industries/zed，mine 指向本 fork
git fetch origin
git rebase origin/main # 我们的提交集中在 file_manager 与少量集成点，冲突通常很少
```

本仓库的提交只触碰上表那些文件；`crates/file_manager` 全部是新增文件，
因此合并上游改动时一般不需要处理该 crate 的冲突。上游原始 README 保留在
[`README.upstream.md`](README.upstream.md)。

## 8. 许可

- 上游 Zed：GPL-3.0 / Apache-2.0 双许可（见 `LICENSE-GPL`、`LICENSE-APACHE`）
- `crates/file_manager`：`GPL-3.0-or-later`（见其 `Cargo.toml` 与 `LICENSE-GPL`）

---

## 附录：CI

`.github/workflows/file-manager.yml`：

- **每次 push / PR**（改动落在 `crates/file_manager` 或其依赖时触发）：
  `./script/linux` 装系统依赖 → 按 `rust-toolchain.toml` 装 Rust 1.98.1 →
  `cargo test -p file_manager --locked`。不下载 WebRTC，标准 runner 跑得动。
- **手动触发**（Actions → *File manager* → Run workflow，勾选 `build_app`）：
  `cargo build -p zed` 并上传 `target/debug/zed` 工件（约 40 分钟）。

> 本仓库带着上游那几十个 workflow，它们大多需要 secrets / self-hosted runner。
> 建议只保留上面这一个：`gh workflow list --all --json name --jq '.[].name'` 后逐个
> `gh workflow disable <name>`。
