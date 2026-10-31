# 用 root 验证全量 PSS

非 root 下 `smaps_rollup` 只对本用户进程可读（内核 ptrace 检查）。这份清单用来确认 root 路径真的拿到了全量，以及"读不到"的两类原因被正确区分。

## 0. 前置检查（先确认 sudo 理论上能不能生效）

```sh
id -u                                   # 非 root 基线
cat /proc/sys/kernel/yama/ptrace_scope  # 2 或 3 会限制；root 通常有 CAP_SYS_PTRACE 可绕过 1/2
grep -E ' /proc ' /proc/mounts          # 不能有 hidepid=2（否则非本用户进程直接不可见）
capsh --print 2>/dev/null | grep -i ptrace   # 容器里可能被 drop 掉 CAP_SYS_PTRACE
```

本机基线：`ptrace_scope=1`、`/proc` 无 hidepid、非 root。

## 1. 非 root 基线（先跑一遍，记下数字）

```sh
./target/release/mtop --json --fields sampling > /tmp/mtop-user.json
cat /tmp/mtop-user.json
```

本机实测（439 个 PID）：

| 字段 | 值 | 含义 |
|---|---|---|
| `procs` | 439 | 全部可见进程 |
| `no_mm` | 176 | 内核线程：无地址空间，**root 也读不到** |
| `denied` | 59 | 其他用户的进程：**root 可读** |
| `covered` | 204 | 已有 PSS 的进程 |
| 表头 | `coverage 203/262 · 59 n/a (needs root)` | 分母 = procs − no_mm |

## 2. root 运行

```sh
sudo ./target/release/mtop
```

**预期变化**（与上表对照）：

| 观察点 | 非 root | root 期望 |
|---|---|---|
| 表头 `n/a (needs root)` | 59 | **0**（提示消失） |
| `covered` / 分母 | 203/262 | **265/265**（进程数会波动） |
| `no_mm` 行（内核线程） | 显示 `n/a` | 仍是 `n/a`，但不再有 sudo 提示 |
| 进程详情（对 root 进程按 `Enter`） | "vm mappings unavailable … PermissionDenied" | 正常显示 VMA |

非 root 进程（如 chrome、你自己的服务）的 PSS 数字**不应该变化**——若变化，说明读的是不同的量，属 bug。

## 3. 自检脚本（断言不靠眼睛）

```sh
sudo scripts/verify-root.sh
```

脚本自己取 `--json`（只投影需要的字段），打印计数并断言：

```
procs=445 covered=265 denied=0 no_mm=180
pss total=6.28 GiB  pss<=rss violations=0
OK
```

- `denied` 必须为 0：否则仍有进程返回 EACCES，按第 4 节排查。
- `violations` 必须为 0：PSS 是 RSS 按共享度折减后的值，不可能大于 RSS。
- `covered + no_mm + denied` 应等于 `procs`（允许少量进程在采样期间来去）。

非 root 跑同一脚本会明确失败，可用来确认断言确实生效：

```
procs=442 covered=203 denied=59 no_mm=180
FAIL: 59 processes still return EACCES; see docs/VERIFY-ROOT.md section 4
FAILED
```

## 4. 如果 root 下仍有 `denied`

按可能性排查：

1. **容器 drop 了 `CAP_SYS_PTRACE`**：`sudo` 只给 uid 0，不给能力。用 `capsh --print` 确认。
2. **Yama `ptrace_scope=3`**：该级别对所有进程生效，包括 root。
3. **`/proc` 以 `hidepid=1|2` 挂载**：此时那些进程根本不出现在列表里（`procs` 变少），而不是标 `n/a`。
4. **SELinux/AppArmor 限制**：`dmesg | tail` 里会有 `avc: denied`。
5. **PID 命名空间不同**：`sudo` 在另一个 namespace 里看到的是另一批 PID，数字对不上属正常。

## 5. 清理

脚本用 `mktemp` 并在退出时删除临时文件，无需手动清理。

## 6. 实测记录（2026-10-31）

root：

```
$ sudo scripts/verify-root.sh
procs=447 covered=264 denied=0 no_mm=183
pss total=6.27 GiB  pss<=rss violations=0
OK
```

- `264 + 183 = 447`：每个进程都被归入"已覆盖"或"无 mm"，没有第三种状态。
- `denied=0`：非 root 下那 ~59 个 EACCES 全部转为可读。
- TUI 同次验证：表头 `coverage 265/265`，无 `n/a (needs root)` 提示；`dnf5daemon-serv` 从 `n/a` 变为 PSS 330M；用户自己的进程（chrome 339M / omp 351M）PSS 不变——root 只打开了权限，没有改变统计口径。

非 root（同一脚本，负向用例）：

```
procs=442 covered=203 denied=59 no_mm=180
FAIL: 59 processes still return EACCES; see docs/VERIFY-ROOT.md section 4
FAILED
```
