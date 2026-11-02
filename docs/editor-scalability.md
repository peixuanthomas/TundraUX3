# 编辑器大文件支持

编辑器将高亮、软换行、搜索和行变更标记的大文件计算放在后台，并按视口或数据块缓存结果，避免在主事件循环中重复扫描全文。

## 计算与缓存

- **语法高亮：**视口只保留所需逻辑行的 token，每 128 行保存一次解析状态，最多 2,048 个检查点。编辑后复用未受影响的前缀；首次跳到文件末尾时，后台解析前缀以恢复多行注释和字符串状态。
- **软换行：**后台建立逻辑行的累计显示行数，显示行按需生成。ASCII 可直接计算位置；长 Unicode 行每 256 个显示行保存一个字节检查点。改变宽度后重建索引。
- **搜索：**普通查询以 64 KiB 窗口扫描 Rope，并保留查询长度和 Unicode 单词边界上下文。交互搜索缓存最多 512 个匹配位置，完整计数继续累计；F3 在页内复用结果。
- **替换：**预览使用同一分块引擎，应用前核对修订号、文档代次、原文和字素边界，整批替换一次撤销。全量预览最多保留 10,000 处或 8 MiB 数据。
- **后台任务：**文件超过 64 KiB 时使用后台线程。每项功能最多保留一个运行任务和一个待处理任务，新请求取消旧请求，过期结果不回写。
- **行变更标记：**后台比较先跳过未变化的前后缀。单个超长 ASCII 行的字数增量只检查编辑两侧的固定上下文。

## 复现基准

```sh
cargo build --release -p app --example editor_scalability
python3 scripts/benchmark-editor.py > editor.csv 2> editor-rss.csv
python3 scripts/benchmark-editor.py --sizes 1 10 --fixtures dense > dense.csv 2> dense-rss.csv
```

默认矩阵包含 1、10、100 MiB 文件，以及 ASCII 极长单行、ASCII 日志、中文、Emoji 和 CRLF 日志。每个场景使用独立进程，并依次测量首屏高亮、末尾高亮、软换行、搜索、编辑后的重排与行标记。

指标含义：

- `request_ms`：调用返回前的主线程耗时。
- `total_ms`：包含后台完成与 1ms 轮询间隔的总耗时。
- `extra_peak_heap_mib`：相对操作开始时已存活分配的峰值增量。
- Linux `VmHWM`：整个进程的峰值 RSS，包括已打开文件。

## 参考结果

2026-10-13，Linux x86_64、AMD EPYC 9V74、Rust 1.99.0，release 构建。以下是一次运行的观测值，用于比较实现，不作为 CI 耗时门槛。

| ASCII 极长单行 | 搜索请求（ms） | 搜索完成（ms） | 搜索额外峰值堆（MiB） |
| --- | ---: | ---: | ---: |
| 1 MiB | 0.137 | 2.288 | 0.124 |
| 10 MiB | 0.179 | 4.463 | 0.128 |
| 100 MiB | 0.135 | 25.403 | 0.128 |

| 100 MiB 场景 | 换行请求（ms） | 换行完成（ms） | 编辑及重排请求（ms） | 编辑及重排完成（ms） | 峰值 RSS（MiB） |
| --- | ---: | ---: | ---: | ---: | ---: |
| ASCII 极长单行 | 0.018 | 3.235 | 3.067 | 6.775 | 216.5 |
| ASCII 日志 | 0.048 | 860.124 | 0.434 | 398.413 | 277.8 |
| 中文 | 0.039 | 3784.134 | 0.361 | 254.921 | 217.0 |
| Emoji | 0.059 | 3591.546 | 0.345 | 312.699 | 263.6 |
| CRLF | 0.042 | 755.571 | 0.340 | 311.711 | 274.0 |

10 MiB 密集场景包含 5,242,880 个匹配，交互缓存保留 512 个位置；搜索额外峰值堆为 0.167 MiB，完整计数耗时 1.39 秒，主线程请求耗时 0.110ms。

原始数据见[完整矩阵](benchmarks/editor-scalability.csv)、[峰值 RSS](benchmarks/editor-scalability-rss.csv) 和[密集匹配](benchmarks/editor-dense.csv)。

## 实现边界

- 正则表达式保留跨行、锚点和捕获组语义，后台构造连续字符串后执行；取消在一次引擎调用返回后生效。
- 超过 256 KiB 的逻辑行及其后续内容不做语法着色，文本仍可正常编辑和搜索。
- 换行索引和行变更标记按逻辑行存储，远距离高亮和首轮索引可能需要数秒后台计算。
- `SearchEngine::find_all`、`find_all_rope` 和全文高亮接口返回全部结果；交互界面使用后台视口与分页接口。

## 回归检查

```sh
cargo test --locked -p app
cargo test --locked -p shell --lib editor
cargo test --locked -p shell --test editor
cargo test --locked -p ui --test editor
cargo check --workspace --all-targets --all-features --locked
python3 scripts/check-localization.py
```

`editor_scalability` 覆盖分块与连续搜索等价、Unicode 边界、密集分页、循环导航、过期任务、替换预览、长行换行，以及编辑、撤销和宽度变化后的缓存一致性。
