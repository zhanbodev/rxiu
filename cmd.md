# rxiu 命令手册

## 基本命令

| 命令 | 语法 | 说明 |
|------|------|------|
| `create` | `create <zone_name>` | 创建一个新的文件区域 (zone) |
| `use` | `use <zone_name>` | 切换到指定的文件区域 |
| `list` | `list` | 列出当前区域中的所有文件 |
| `list area` | `list area` | 列出所有已创建的文件区域 |
| `list storage` | `list storage` | 显示文件在本地磁盘的存储位置 |

## 文件操作

| 命令 | 语法 | 说明 |
|------|------|------|
| `put` | `put [name]` | 导入本地文件到当前区域 |
| `get` | `get <file_name>` | 从当前区域导出文件到本地 |
| `del` | `del <file_name>` | 从当前区域删除指定文件 |

## P2P 远程操作 ⭐

| 命令 | 语法 | 说明 |
|------|------|------|
| `peers` | `peers` | 显示已发现的局域网节点列表 |
| `ruse` | `ruse <n>` | 选择第 n 号节点连接 |
| `ruse` | `ruse <zone>` | 选择远程节点的某个 zone |
| `ruse` | `ruse` | 查看当前连接状态 |
| `rarea` | `rarea` | 列出当前连接节点的所有 zones |
| `rlist` | `rlist` | 列出当前选中 zone 的文件 |
| `rget` | `rget <n>` | 下载第 n 号文件 |

### P2P 使用流程 🚀

```bash
# 1. 查看局域网内发现的节点
> peers

LAN PEERS (1)
────────────────────────────────────────────────────────────
  [1] 12D3KooWA1Bc - /ip4/192.168.1.100/tcp/51234

# 2. 选择 1 号节点
> ruse 1
✓ Connected to peer [1] 12D3KooWA1Bc
Use 'rarea' to see zones.

# 3. 列出远程 zones
> rarea

REMOTE ZONES
──────────────────────────────────────────────────
  📁 documents
  📁 photos

# 4. 选择 documents zone
> ruse documents
✓ Selected zone: documents
Use 'rlist' to see files.

# 5. 列出文件
> rlist

📁 documents (2 files)
──────────────────────────────────────────────────
  [1] 📄 report.pdf (2.5 MB)
  [2] 📄 notes.txt (1.2 KB)

# 6. 下载 1 号文件
> rget 1
# → 弹出目录选择器，选择保存位置
# → 后台开始下载，状态栏显示进度
# → 完成后显示结果
✅ Downloaded 'report.pdf' to /Downloads/report.pdf
   2621440 bytes, hash: a1b2c3d4...
```

### 快捷操作

```bash
# 直接指定 zone 列出文件（如果已选择节点）
> rlist documents

# 查看当前连接状态
> ruse
REMOTE CONNECTION STATUS
──────────────────────────────────────────────────
  Peer: [1] 12D3KooWA1Bc
  Zone: documents
  Cached files: 2
```

## RS (Block Sharing) 命令

| 命令 | 语法 | 说明 |
|------|------|------|
| `rs` | `rs` | 进入 RS 模式 |
| `rslist` | `rslist` | 列出 RS 共享文件 |
| `rsput` | `rsput [name]` | 共享文件到 RS 空间 |
| `rsget` | `rsget <number>` | 按序号下载 RS 文件 |
| `rsget` | `rsget <file_name>` | 按文件名下载 RS 文件 |
| `rsdel` | `rsdel <number>` | 删除 RS 文件（同步删除） |
| `rsdel` | `rsdel <file_name>` | 删除 RS 文件（同步删除） |
| `rsstatus` | `rsstatus` | 查看 RS 模式/同步/传输状态 |
| `rsstats` | `rsstats` | 查看 RS 本地统计信息 |
| `rshave` | `rshave <number>` | 查看本机拥有的块 |
| `rshave` | `rshave <file_name>` | 查看本机拥有的块 |
| `rspeers` | `rspeers` | 查看 RS 节点列表 |
| `rsprogress` | `rsprogress` | 查看 RS 传输/同步进度 |
| `rscfg` | `rscfg show` | 查看 RS 配置 |
| `rscfg` | `rscfg concurrency <2-16>` | 调整 RS 下载并发数 |
| `rscfg` | `rscfg sync_concurrency <2-16>` | 调整 RS 同步并发数 |
| `rscfg` | `rscfg block_size <4-32>` | 调整 RS 块大小（MB） |
| `rscfg` | `rscfg gsyn <0|1>` | 全局同步开关（0=仅 RS 模式，1=始终同步） |
| `rxiu` | `rxiu` | 回到默认模式 |

说明:
- `gsyn=1`: 守护进程后台持续同步，即使关闭 TUI 也会继续。
- `gsyn=0`: 守护进程不自动同步，进入 RS 模式时由 TUI 触发同步。

## 其他命令

| 命令 | 语法 | 说明 |
|------|------|------|
| `help` | `help` 或 `?` | 显示帮助信息 |
| `exit` | `exit` 或 `quit` 或 `q` | 退出程序 |

---

## 进度条显示 📊

下载大文件时，状态栏会显示传输进度：

```
 ⬇ download report.pdf 150 MB/500 MB 30% [██████░░░░░░░░░░░░░░]
```

- **⬇** — 下载中
- **⬆** — 上传中  
- 进度条实时更新，不会阻塞界面操作

---

## 文件浏览器快捷键

在文件浏览器模式中：

| 按键 | 作用 |
|------|------|
| `j` / `↓` | 向下移动 |
| `k` / `↑` | 向上移动 |
| `Enter` / `l` | 进入目录 |
| `h` / `Backspace` | 返回上级目录 |
| `y` | 确认选择 |
| `q` / `Esc` | 取消操作 |

---

## 存储位置

- **macOS/Linux**: `~/.rxiu/zones/<zone_name>/files/`
- **Windows**: `C:\Users\<用户名>\.rxiu\zones\<zone_name>\files\`

使用 `list storage` 命令可查看详细路径。
