# mtop

Linux 内存分析 TUI：按 RSS / **PSS** / **USS** 看进程，按映射聚合下钻到 VMA。

`htop` 只给 RSS；把共享库算 N 遍的"内存排行"会骗人。mtop 用内核的 PSS 口径回答"谁真的占了内存"，并给出每个进程的内存花在了哪些映射上。

```
 mtop  466 procs  mem 9.51G/15.36G  avail 5.85G  cached 6.03G  swap 3.25G/16.00G
 sweep 24ms pss 155ms coverage 77/466 = 6.59G age 325ms              ┌ 579585 chrome 530M ┐
┌ procs  sort RSS ↓  filter - ───────────────────────────────────────│▇▇▇▇█               │
│     PID COMM                   RSS     PSS     USS PSS/RSS     Δ/s  │█████               │
│▸ 579585 chrome                530M    348M    321M     66% +  0.0M  └─────────────────────┘
│  623313 omp                   395M    349M    325M     88% +  0.3M
│  582966 ChatGPT               353M    243M    219M     69%      0
```

## 构建与运行

```sh
cargo build --release
./target/release/mtop            # 全系统
./target/release/mtop --pid 1234 # 启动即选中某个进程
```

## 按键

| 键 | 作用 |
|---|---|
| `j`/`k`、`↑`/`↓`、`PgUp`/`PgDn` | 移动 |
| `g`/`G` | 顶 / 底 |
| `s` / `r` | 切换排序键 / 反向 |
| `f` | 过滤（进程名子串或 PID 前缀），`Enter` 应用，`Esc` 取消 |
| `Enter` | 进入 / 退出 VMA 详情 |
| `Esc` | 返回 / 清除过滤 |
| `q` | 退出 |

## 设计要点

两层采样，按实测成本分配预算：

| 路径 | 成本 | 频率 |
|---|---|---|
| `/proc/*/statm` 全量 | 15–25 ms（负载高时 ~40 ms） | 1 Hz |
| `/proc/PID/smaps_rollup` | 1.5 ms 均值，大进程 16–19 ms | 受 150 ms/tick 预算约束 |
| `/proc/PID/smaps` | 31–65 ms（2000+ VMA） | 仅焦点进程，2 s 一次 |

`/proc` 解析全部交给 [`procfs`](https://crates.io/crates/procfs) 0.18，UI 用 [`ratatui`](https://crates.io/crates/ratatui) 0.30（版本锁定）。复用了什么、为什么没有 fork `bottom`、以及精度与风险，见 [docs/REQUIREMENTS.md](docs/REQUIREMENTS.md)。

## 测试

```sh
cargo test --release
```

- `tests/procfs.rs`：对真实 `/proc` 的不变量（PSS ≤ RSS、分组求和、meminfo 单位、statm 与 rollup 一致性）
- `tests/render.rs`：`TestBackend` 把 TUI 渲染成纯文本后断言列、排序、截断、`—`、覆盖率、详情聚合、按键边界
