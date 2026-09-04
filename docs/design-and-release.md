# 设计要点与在线打包说明

> 自 README 迁出（2026-09-04）。README 只保留项目简介 / 构建 / 使用入口，设计细节与 CI 打包说明集中在本文件。

## 在线自动打包（GitHub Actions，发布 Release 触发）

`.github/workflows/release.yml` 在**发布 GitHub Release（点 Publish）时**，用 GitHub 托管 runner 自动构建三端安装包并上传为 Release 附件，无需本地机器：

| 平台 | runner | 产物 |
|---|---|---|
| macOS（Apple Silicon） | `macos-latest` | `.dmg`（含 `.app`） |
| Windows（x64） | `windows-latest` | `.exe`（NSIS 安装器） |
| Linux（x64） | `ubuntu-22.04` | `.deb` + `.AppImage` |

**发布流程**：

```bash
# 1. 把三处 version 对齐（当前 0.1.0）：tauri.conf.json / Cargo.toml / package.json
# 2. 打 tag 并推送
git tag v0.1.0 && git push origin v0.1.0
# 3. 在 GitHub 上基于该 tag 创建 Release → Publish → 自动触发打包
#    （也可在 Actions 页手动 Run workflow 兜底）
```

**前提说明**：

- 已在 `tauri.conf.json` 的 `bundle.icon` 加入 `icons/icon.ico`（Windows 安装器图标必需，从 `icon-512.png` 生成）。
- 三端产物**均未做代码签名**：macOS 首次打开需右键「打开」/ `xattr -cr`；Windows 会弹 SmartScreen 提示（点「更多信息 → 仍要运行」）；Linux 无签名要求。
- 本仓库是**私有/个人仓库**，`GITHUB_TOKEN` 已授予 `contents: write` 用于上传附件；公开仓库需注意 Release 附件对外可见。
- 如日后需要 Intel Mac，在 workflow 的 matrix 里给 `macos-latest` 增加 `--target x86_64-apple-darwin` 一项即可。

## 设计要点（纯本地，与 GitHub 解耦）

- **拉取**：`gh api` 调 Search API，**合并多个稳定查询源按 `repo#number` 去重**（已排除 PR）：`assignee:<login>`（分配给我，权威）+ `author:<login>`（我创建）+ `mentions:<login>`（@我）+ `commenter:<login>`（我评论）+ `involves:<login>`（兜底）。
  - ⚠️ **为何多源而非单一 `involves:`**：GitHub `involves:` 搜索结果**非确定性抖动**——总数恒为 76，但成员会随机漏拉（如 `fad-backend#1066/#1071/#1072/#1100/#1138/#1139`、`pq-backend#259` 曾在某次 `involves` 结果中缺失）。多个稳定源取并集，任一源漏拉都会被其他源补回，看板不再随机缺任务。
- **归属三分**：无 assignee → `notassignee`；含我 → `assigned`；他人 → `assigned-others`（GitHub Search 不支持 `-no:assignee`，须读 `assignees` 数组判定，不能用查询语法区分）
- **状态四态**：`todo` / `doing` / `processed` / `done`，本地维护，同步时不被远程覆盖
- **closed 处理**：GitHub 已关闭的任务 → 状态置「已完成」(`done`)，并标 `candidate_done` 作「远程已关闭、待本地确认」提示；以远程真实状态为权威，覆盖本地手动态（即便曾被标为处理中/已处理，关闭即视为做完）
- **PR 关联**：卡片记录对应 PR 的编号与链接。实现上**不使用 Search API 的 `is:pr`**（限流极严、突发易挂起拖垮同步），改为按「用户 issue 实际所在仓库」调用 REST `repos/{org}/{repo}/pulls?state=all` 拉取各仓库 PR，解析正文 `#N` / `owner/repo#N` 反向关联到看板卡片（`best-effort`：单页/单仓失败仅跳过，不影响其余）
- **定时**：应用内 ticker（应用常驻菜单栏时持续生效；退出应用即停止）
