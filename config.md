# rxiu 配置文件说明

## 配置文件位置

```
~/.rxiu/config/config.toml
```

## 配置项

| 参数 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `rs_concurrency` | 整数 | `8` | RS 下载并发数 |
| `rs_sync_concurrency` | 整数 | `8` | RS 后台同步并发数 |
| `rs_block_size_mb` | 整数 | `16` | RS 文件块大小（MB） |
| `rs_global_sync` | 布尔 | `true` | 全局同步开关 |
| `rs_replication_factor` | 整数 | `2` | 副本因子 |
| `renew_enabled` | 布尔 | `true` | P2P 自动更新开关 |
| `renew_check_interval` | 整数 | `300` | 自动更新检查间隔（秒） |

---

## 详细说明

### `rs_concurrency`

RS 文件下载时的并发任务数。

- **范围**: 2 ~ 16
- **建议**: 局域网环境可适当调高（8-12），网络较差时调低

### `rs_sync_concurrency`

后台同步缺失块时的并发任务数。

- **范围**: 2 ~ 16
- **建议**: 保持较低值（4-6）以避免影响前台操作

### `rs_block_size_mb`

上传文件时的分块大小（MB）。

- **范围**: 4 ~ 32
- **注意**: 已上传的文件不会因修改此配置而重新分块

### `rs_global_sync`

控制 daemon 是否在后台持续同步。

| 值 | 行为 |
|-----|------|
| `true` | 后台持续同步，即使关闭 TUI 也会继续 |
| `false` | 仅在进入 RS 模式时触发同步 |

### `rs_replication_factor`

每个块存储在多少个节点上。

| 值 | 含义 | 容错能力 |
|----|------|----------|
| `1` | 无冗余 | 任意节点离线可能导致数据无法访问 |
| `2` | 双副本（推荐） | 可容忍 1 个节点离线 |
| `N` | N 副本 | 可容忍 N-1 个节点离线 |

**空间占用**（3 节点示例）:
- `1`: 每节点存 1/3 文件
- `2`: 每节点存 2/3 文件
- `3`: 每节点存完整文件

### `renew_enabled`

是否启用 P2P 自动更新功能。

| 值 | 行为 |
|-----|------|
| `true` | daemon 定期检查其他节点是否有新版本，自动下载并更新 |
| `false` | 关闭自动更新 |

### `renew_check_interval`

自动更新检查间隔（秒）。

- **默认**: 300（5 分钟）
- **建议**: 生产环境可适当调高（600-1800）

---

## 热加载

配置文件修改后 **2 秒内自动生效**，无需重启 daemon。

## 示例配置

```toml
rs_concurrency = 8
rs_sync_concurrency = 4
rs_block_size_mb = 16
rs_global_sync = true
rs_replication_factor = 2
renew_enabled = true
renew_check_interval = 300
```

