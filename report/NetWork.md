# RXIU 网络机制 / 连接机制 / 协议总览

本文件基于当前代码实现，列出 RXIU 的网络层机制、连接流程、协议定义与传输路径，便于排查多节点环境下的协议问题。

## 1. 架构与组件

RXIU 运行时由三个主要网络相关组件构成：

- **TUI/CLI 前端进程**: 用户交互入口（`src/main.rs`, `src/tui/*`, `src/cli/*`）。
- **后台守护进程**: 负责 P2P 网络与 RS 同步（`src/bin/rxiu-daemon.rs`, `src/daemon/*`）。
- **P2P 网络节点**: 基于 libp2p，提供发现/连接/请求响应协议（`src/p2p/*`）。

数据存储相关模块：

- **Zone 文件存储**: `src/storage/*`（本地文件区）。
- **RS 分块存储**: `src/rs/*`（块级存储与校验）。

## 2. 连接机制

### 2.1 TUI/CLI → Daemon（本地 IPC）

- **传输方式**: TCP 本地环回（`127.0.0.1`）
- **端口**: `19820`（`DAEMON_PORT` in `src/daemon/protocol.rs`）
- **协议封装**: 4 字节长度前缀 + CBOR 序列化（`encode_message` / `parse_length`）
- **用途**: TUI/CLI 通过 `DaemonRequest` 调用后台 P2P 功能、RS 功能

相关文件：
- `src/daemon/protocol.rs`
- `src/daemon/client.rs`
- `src/daemon/server.rs`
- `src/daemon/proxy.rs`

### 2.2 P2P 节点发现与连接

- **发现机制**: mDNS 局域网发现（`libp2p::mdns`）
- **监听地址**: `0.0.0.0:0`（随机 TCP 端口）
- **连接流程**:
  1. mDNS 发现 peer → 立即 dial
  2. 建立连接后加入 `discovered_peers`
  3. 主动请求对方已知 peers（Peer Exchange）

相关文件：
- `src/p2p/node.rs`
- `src/p2p/service.rs`
- `src/p2p/protocol/peer_exchange.rs`

### 2.3 Peer Exchange（节点扩散）

- 触发时机: 发现新 peer 后发送 `GetPeers` 请求
- 远端返回 `Peers(Vec<PeerEntry>)`
- 本地会对每个 peer 执行 dial（跳过已知/自己）

相关逻辑：
- `FileRequest::GetPeers` / `FileResponse::Peers`
- `NodeEvent::PeersReceived` → `node.dial_peer(...)`

### 2.4 心跳与网络恢复

- **心跳机制**: 每 5 秒发送 `Ping` 给所有已知 peer
- **失效判定**: 连续 4 次未响应（约 20s）移除 peer
- **网络恢复**:
  - 休眠/唤醒检测（时间跳变）
  - peer=0 时周期性刷新（60s）
  - stale 检测 + 重试策略

相关文件：
- `src/p2p/service.rs`
- `src/p2p/recovery.rs`

## 3. P2P 协议（File Protocol）

### 3.1 协议基本信息

- **协议名称**: `/rxiu/file/1.0.0`
- **传输模型**: libp2p request-response
- **编码**: CBOR（`cbor4ii`）
- **默认限制**:
  - 请求最大 1MB（`REQUEST_SIZE_MAXIMUM`）
  - 响应最大 64MB（`RESPONSE_SIZE_MAXIMUM`）

相关文件：
- `src/p2p/messages.rs`
- `src/p2p/codec.rs`
- `src/p2p/node.rs`

### 3.2 请求类型（FileRequest）

- `Ping`
- `ListZones`
- `ListFiles { zone }`
- `GetFile { zone, name }`
- `GetFileMeta { zone, name }`
- `GetFileChunk { zone, name, offset, size }`
- `GetPeers`

RS 模式请求：
- `RsList`
- `RsAnnounce { file }`
- `RsGetMeta { name }`
- `RsGetBlock { hash }`
- `RsGetBlocks { hashes }`
- `RsHave { name }`
- `RsDelete { name }`

### 3.3 响应类型（FileResponse）

- `Pong`
- `Zones(Vec<String>)`
- `Files { zone, files }`
- `FileData { name, content, hash }`
- `FileMeta(FileMeta)`
- `FileChunk(FileChunk)`
- `Peers(Vec<PeerEntry>)`

RS 模式响应：
- `RsFiles(Vec<RsFileEntry>)`
- `RsMeta(RsFileEntry)`
- `RsBlock(RsBlock)`
- `RsBlocks(Vec<RsBlock>)`
- `RsHave(RsHave)`
- `RsOk`
- `Error(String)`

## 4. Daemon IPC 协议

Daemon 的请求/响应与 P2P 功能一一对应，通过本地 TCP 传输：

- 请求: `DaemonRequest::*`（如 `ListRemoteZones`, `FetchFile`, `RsGetBlock`, `RsSync`）
- 响应: `DaemonResponse::*`（如 `Zones`, `Files`, `FileData`, `RsBlock`）

相关文件：
- `src/daemon/protocol.rs`
- `src/daemon/server.rs`
- `src/daemon/client.rs`
- `src/daemon/proxy.rs`

## 5. RS Block 直连协议（Block Server）

为减少 CBOR/IPC 开销，RS 的 block 下载可走直连端口：

- **端口**: `19821`（`BLOCK_SERVER_PORT`）
- **连接方式**: TCP 直连目标 peer 的 IP
- **协议格式**:
  - 请求: `[4 bytes hash_len] + [hash bytes]`
  - 响应: `[4 bytes status] + [4 bytes data_len] + [data]`
  - status: `0=OK`, `1=NOT_FOUND`, `2=ERROR`

相关文件：
- `src/daemon/block_server.rs`
- `src/tui/block_client.rs`

## 6. 传输流程（摘要）

### 6.1 Zone 文件传输

1. `ListZones` / `ListFiles` 获取目录信息
2. `GetFileMeta` 获取元数据与 chunk 信息
3. `GetFileChunk` 拉取分片（支持断点续传）
4. 校验哈希并写入本地

核心实现：
- `src/p2p/service.rs`（请求/响应处理）
- `src/tui/app.rs`（chunked download + resume）

### 6.2 RS 文件传输

- 元信息：`RsList` / `RsGetMeta`
- 块下载：
  - Daemon 同步时走 `RsGetBlock` / `RsGetBlocks`
  - TUI 下载时优先用 `BlockClient` 直连 block server

核心实现：
- `src/rs/mod.rs` / `src/rs/sync.rs`
- `src/daemon/rs_sync.rs`
- `src/tui/app.rs`

## 7. 关键常量与配置

- `FILE_CHUNK_SIZE = 4MB`（`src/p2p/messages.rs`）
- `DAEMON_PORT = 19820`（`src/daemon/protocol.rs`）
- `BLOCK_SERVER_PORT = 19821`（`src/daemon/block_server.rs`）
- P2P request timeout = 30s（`src/p2p/node.rs`）
- 配置项（`src/config.rs`）:
  - `rs_concurrency`
  - `rs_sync_concurrency`
  - `rs_block_size_mb`
  - `rs_global_sync`

---

如需进一步排查 30+ 节点规模的问题，建议结合：

- `report/performance_optimization.md`
- P2P 心跳/恢复日志（daemon 日志）
- 连接生命周期事件（`SwarmEvent::ConnectionEstablished` / `ConnectionClosed`）
