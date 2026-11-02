# platform 性能与复测

`platform` 的热路径包括进程输出、日志尾部、文档指纹、目录读取和磁盘扫描。实现优先使用系统等待接口、分块处理和有限缓存，同时保留取消、文件身份、链接和挂载检查。

## 实现要点

- **进程等待：**Unix 使用 `poll`，Linux 在可用时使用 `pidfd` 等待退出；已读完的管道退出等待集合。Windows 在空闲时等待进程句柄，每轮最长 20ms 后检查取消。
- **日志分配：**扫描最多 8 MiB 尾部时只保存所需行的位置，再创建日志对象，避免为旧行分配事件、时间戳和路径字符串。
- **文档指纹：**使用 `xxhash-rust` 的 XXH3-64 分块计算，写入使用 64 KiB 缓冲。XXH3 用于变化检测，发布校验仍使用 SHA-256。
- **恢复记录：**新草稿保存 `saved_content_hash_algorithm = "xxh3-64-v1"`。没有该标记的旧草稿仍可读取正文，保存时需要重新绑定目标文件。
- **目录读取：**普通文件只查询一次属性，ELF 和脚本头合并为一次打开；文件头使用不跟随末级链接的非阻塞读取。
- **磁盘扫描：**Linux 使用一次 `statx` 取得类型、大小、块数、设备、文件和挂载编号。目录游标持有已打开句柄，深度上限为 `min(256, max(1, RLIMIT_NOFILE - 32))`。

## 性能复测

```sh
CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 cargo run --release --locked -p platform --example performance_probe
```

程序只操作临时目录，通过公开接口执行真实进程、文档和目录操作。每项预热一次后取 7 次中位数，并统计 Rust 分配和重新分配次数。

样本包括 9 MiB 日志、8 MiB 尾部读取、200 条返回记录、64 MiB 文档和 3,000 个普通文件。文档读取使用热缓存，结果不代表冷盘或远程文件系统。

固定核心复测：

```sh
CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 cargo build --release --locked -p platform --example performance_probe
taskset --cpu-list 0 target/release/examples/performance_probe
```

## 参考结果

2026-10-11，Intel Core Ultra 9 275HX、WSL2、Rust 1.98.0，release 构建，对比基线 `cf3592e` 与实现 `56928f8`。单位为毫秒。

| 项目 | 优化前 | 优化后 |
| --- | ---: | ---: |
| 等待并收集 4 MiB 进程输出 | 805.870–861.546 | 1.134–1.262 |
| 读取 8 MiB 日志尾部，返回 200 条 | 553.182–565.547 | 12.223–12.788 |
| 64 MiB 文档指纹，含文件读取 | 57.206–57.529 | 5.236–6.082 |
| 64 MiB 文档按 1 KiB 小块保存，含同步 | 108.130–111.699 | 38.148–38.684 |
| 列出 3,000 个普通文件及打开策略 | 8.625–9.261 | 4.692–4.769 |
| 扫描 3,000 个文件并生成结果 | 5.189–5.397 | 4.610–4.687 |

日志查询的 Rust 分配次数从 4,976,297 降到 7,031，扫描从 53,952 降到 47,948。进程等待的差异主要来自移除固定 20ms 停顿，不代表所有程序都有相同比例的收益。

## 回归检查

```sh
cargo test --locked -p platform
cargo test --locked -p platform management::disks::scan:: -- --include-ignored --test-threads=1
cargo fmt --all --check
git diff --check
```

专项覆盖双管道读取、取消、日志截断、不同写入块、失败保留原文件、旧草稿、符号链接、硬链接、绑定挂载、目录改名和低句柄限制。隔离挂载测试使用独立用户与挂载命名空间。
