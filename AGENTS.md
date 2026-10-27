# AGENTS.md

给在本仓库干活的 AI 编程助手（agent）看的入口。规则的正本是 `CONTRIBUTING.md`，这里不重复。

1. **先读 `CONTRIBUTING.md`**：硬约束（只用 Rust、不用 Calibre、clean-room、书的内容一个字不多一个字不少……）、版本号、提交要求、命令都在里面。
2. **再按任务读 `docs/` 里对应的一篇**（对照表在 `CONTRIBUTING.md#动手前先读`）；定过的事在 `docs/decisions.md`，和它冲突时报告维护者，不要自己改回去。
3. **提交前**：`cargo test --workspace` 全部通过，`cargo clippy --workspace --all-targets -- -D warnings` 0 警告。
4. **推新分支，不直接改 `master`**；影响产物的改动在提交说明里写清"没跑真书回归、没上真机"，不要声称回归通过或真机验证过。
5. 文档、注释、提交说明用中文；提交说明带 `feat:` / `fix:` / `docs:` / `chore(范围):` 前缀，正文写清原因。
