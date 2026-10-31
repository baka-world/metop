# mtop

Linux 内存分析 TUI：按 RSS / **PSS** / **USS** 看进程，按映射聚合下钻到 VMA。

`htop` 只给 RSS；把共享库算 N 遍的"内存排行"会骗人。mtop 用内核的 PSS 口径回答"谁真的占了内存"，并给出每个进程的内存花在了哪些映射上。

```
 mtop  442 procs  mem 8.13G/15.36G  avail 7.23G  cached 6.15G  swap 3.28G/16.00G
 sweep 35ms pss 155ms coverage 205/205 = 5.85G  237 n/a (sudo for full) age 340ms
┌ procs  sort RSS ↓  filter - ─────────────────────────────────────────────────────┐
│     PID COMM                   RSS     PSS     USS PSS/RSS     Δ/s  SHARE  PSS MIX│
│▸ 579585 chrome                524M    346M    320M     66%      0  ██████ ██████████
│  623313 omp                   397M    349M    328M     88% +  0.1M █████  ████████████
│    4551 dnf5daemon-serv       354M     n/a     n/a      —       0  ████
│  512136 jetbrains-toolb       308M    281M    279M     91% -  0.0M ████   ████████████
└──────────────────────────────────────────────────────────────────────────────────┘
 q quit │ j/k move │ g/G ends │ s sort (RSS) │ r reverse │ f filter │ Enter vma detail
```

## 构建与运行

```sh
cargo build --release
./target/release/mtop            # 全系统
./target/release/mtop --pid 1234 # 启动即选中某个进程
./target/release/mtop --json     # 一次性快照（可 diff、可 jq）
./target/release/mtop --json --pid 1234   # 附带该进程的 VMA 明细
mtop --diff a.json b.json        # 比较两份快照，按 |ΔRSS| 排序
```

```sh
mtop --json > a.json; sleep 30; mtop --json > b.json
mtop --diff a.json b.json
```

```
mtop diff  over 4s  (437 -> 438 procs)   15 grew, 3 shrank, 1 new, 0 gone
     PID COMM                          ΔRSS      ΔPSS   RSS now   PSS now  note
  613254 chrome                       -1.2M     -1.2M      348M      171M
    2796 gnome-shell                  +0.2M     +0.2M      288M      235M
```

写快照的那个 `mtop` 进程会被记录为 `self_pid` 并在 diff 中自动排除——否则每次运行都是"new 3 MB / gone 3 MB"的假噪声。

## 按键

| 键 | 作用 |
|---|---|
| `j`/`k`、`↑`/`↓`、`PgUp`/`PgDn` | 移动 |
| `g`/`G` | 顶 / 底 |
| `s` / `r` | 切换排序键 / 反向 |
| `f` | 过滤（进程名子串或 PID 前缀）；详情页中过滤映射名 |
| `Enter` | 进入详情；在详情内展开/收起当前映射（逐 VMA） |
| `z` / `Z`（详情） | 全部收起 / 全部展开 |
| `Esc` | 清除过滤 / 返回 |
| `q` | 退出 |

## 设计要点

两层采样，按实测成本分配预算：

| 路径 | 成本 | 频率 |
|---|---|---|
| `/proc/*/statm` 全量 | 15–25 ms（负载高时 ~40 ms） | 1 Hz |
| `/proc/PID/smaps_rollup` | 1.5 ms 均值，大进程 16–19 ms | 受 150 ms/tick 预算约束 |
| `/proc/PID/smaps` | 31–65 ms（2000+ VMA） | 仅焦点进程，2 s 一次 |

`/proc` 解析全部交给 [`procfs`](https://crates.io/crates/procfs) 0.18，UI 用 [`ratatui`](https://crates.io/crates/ratatui) 0.30（版本锁定）。复用了什么、为什么没有 fork `bottom`、以及精度与风险，见 [docs/REQUIREMENTS.md](docs/REQUIREMENTS.md)。

## 详情页看什么

表格里的 `PSS MIX` 是按 PSS 加权的构成条：紫=anon、青=file、黄=shmem、蓝=swap（`NO_COLOR` 生效时只剩单色块，这是 crossterm 的标准行为）。

`Enter` 进入某个进程后，按映射聚合（2292 个 VMA → 286 组），并给出**PSS 构成**数值：anon / file / shmem / swap-pss / THP / KSM。

注意口径：`Pss_Anon`/`Pss_File`/`Pss_Shmem` **只在 `smaps_rollup` 中存在**，逐 VMA 的 `smaps` 不输出它们——所以构成取进程级 rollup，映射级用 `Anonymous`/`Swap`/`Pss`。用逐 VMA 求和会得到全 0（踩过）。

## 已知限制

- **PSS 需要权限**：`/proc/PID/smaps_rollup` 走 ptrace 读权限检查。普通用户只能读到自己（及同凭据 / dumpable）的进程——实测 216/218 之外还有 234 个进程不可读，表头显示 `n/a` 并提示 `sudo for full`。`sudo ./target/release/mtop` 可得全量。
- **读不到不代表不占内存**：`n/a` 的行仍有 RSS，只是没有 PSS。
- 内核线程的 `smaps_rollup` 为空，恒为 `n/a`。

## 测试

```sh
cargo test --release
```

- `tests/procfs.rs`：对真实 `/proc` 的不变量（PSS ≤ RSS、分组求和、meminfo 单位、statm 与 rollup 一致性）
- `tests/render.rs`：`TestBackend` 把 TUI 渲染成纯文本后断言列、排序、截断、`—`、覆盖率、详情聚合、展开、按键边界
- `tests/json.rs` / `tests/diff.rs`：通过真实二进制校验快照与 diff 的对外契约
