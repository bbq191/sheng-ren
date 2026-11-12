# 汉王产物目录改成 `Documents`：只差大小写的目录变更不重传、不删书

- Task ID: 2026-11-12-hanvon-documents-case
- 负责 bot: sheng-ren
- 状态: done
- 执行方: opencode-local
- 分支: hanvon-documents-case（从最新 master `73c095c` 切出）
- 更新时间: 2026-11-12T10:05+08:00
- 引用: 无

## 需求原文
> （vinci 2026-11-11）汉王目录下有两个documents
> （bot 解释是同一个目录、删哪个都可能连书一起删，彻底解决要把汉王的目录名改成跟设备一致的 `Documents`，问要不要写任务单）要

现场（2026-11-11 在 vinci 电脑上核实）：
- 汉王 Clear 6 Turbo 经 jmtpfs 挂在 `/run/user/1000/mtp/hanvon`。`内部共享存储空间/` 下 MTP 列出**两个**目录：`documents`（inode 3）和 `Documents`（inode 101），内容一模一样（12 个子目录）。
- adb 看设备上只有一个：`/sdcard/Documents` 和 `/sdcard/documents` 是同一个 inode 20337（安卓存储不分大小写）。
- 原因：`crates/profile/profiles/hanvon.toml:42` 写的是 `dir = "documents"`（注释里写了设备上是 `Documents`）。booklib 按小写名字往里写，MTP 就给同一个文件夹多登记了一个小写的对象。
- 危险：经 MTP 删"多出来"的那个，很可能把真的目录连书一起删掉。**本任务不删、不改设备上任何东西**，只改 booklib。

## 输入

### 记录怎么存位置（负责 bot 读代码 + 实验得出）
- 生成记录 `output-state/<设备>.json`：每本 `StateEntry { path, root, fingerprint, sha, old, audit }`，`path` 是**绝对路径**、带上 `documents` 这一段，`root` 是产物根目录 `<挂载点>/<存储>/documents`（`crates/library/src/generate.rs:1124-1148`）；`orphans` 同样是绝对路径（`:1116-1121`）。
- 产物根目录由 `deliver::connect` 拼：`storage.join(dir)`（`crates/library/src/deliver.rs:109-121`），`output_dir` 再拼子目录（`generate.rs:343-386`）。设备连接按设备 id 缓存一轮（`crates/library/src/session.rs:191-206`）。
- `[deliver]` 不进指纹（`docs/devices.md:188`「不影响产物内容，不进指纹」），所以改 `dir` 不变指纹、不需要升 `OPTIMIZE_VERSION`，`fp_golden.txt` 不变。

### 只把 `dir` 改成 `Documents` 会怎样（危险，已复现）
走 `prepare_file`（`generate.rs:693` 起）：
1. 新的 `out` 是 `…/Documents/<子目录>/x.epub`，和记录里的 `…/documents/<子目录>/x.epub` 不相等 → 不算最新（`:732-733`）。
2. 指纹没变、旧文件在（经 MTP 小写名也读得到）→ 当成"内容没变、只是挪位置"，拿旧文件当来源（`:767`、`made()`），旧位置记进 `old`（`:791-796`），交给传输线程。
3. 传输 `deliver::put_file(src=…/documents/…/x.epub, dest=…/Documents/…/x.epub, keep_src=false, compare=true, direct=true)`：两个路径父目录不同，`case_twin` 只认**同一目录里**只差大小写（`generate.rs:1429-1438`），所以不走改名保护（`deliver.rs:198-206`）；进 `put_file_once`：`same_bytes(src, dest)` 为真（设备上是同一个文件）→ **`remove_file(src)` 删掉 src**（`deliver.rs:374-377`），在汉王上这就是刚"放好"的那本书。没走到那一步的话，`rename(src, dest)`（`:382`）在 jmtpfs 上也是同一个对象挪到自己身上，结果说不准。
4. 传完 `finish` → `delete_placements(old)`（`generate.rs:1054-1076`、`:1191` 起）：大小写保护同样只看同一目录（`:1203-1225`），会 `remove_product(…/documents/…/x.epub)`（`:1231`）——又是删真文件；还会往上删"变空"的子目录（`:1250-1256`）。
5. 负责 bot 在 box 上用临时集成测试复现（`Documents` 做成指向 `documents` 的符号链接，模拟不分大小写的存储）：第一次 sync 写好；把 `dir` 改成 `Documents` 后第二次 sync 返回挪位置，**目录里的书没了**；第三次才按新书重传。即：每本汉王书都会先被删、再整本重传，进度大概率丢。临时测试没提交。

### 其他设备
- `kindle.toml:75` `documents`：Kindle 设备上本来就是小写 `documents`，没问题，不动。
- `ireader.toml:107-110` 掌阅 `documents`：注释写了设备自带 `Documents`、外部存储 casefold，所以掌阅上**多半也有同样的重影**；`bigme.toml:52` `books`，注释说 xReader 认 `Books/`，同理可能有重影。这两台本任务**不改 `dir`**（vinci 没要求），只靠下面的通用保护保证"以后谁改大小写都不删书"。
- `kobo.toml:54` USB 大容量存储（vfat，不分大小写），`books` 本来就是我们建的，不动。
- 大小写敏感的地方：`deliver.rs:371` 建目录前先 `is_dir()`（`ensure_dir`，`:327` 起），`Documents` 已在就不建；`space.rs:58` `probe_path`只往上找存在的目录，跟大小写无关；`mtp_storage` 认存储名时已经不分大小写（`deliver.rs:157-175`）。真正会出事的只有上面记录比较和删除这两处。

### 涉及文件（预计）
- `crates/profile/profiles/hanvon.toml`（修改：`dir = "Documents"`、注释）
- `crates/profile/src/lib.rs`（修改：内置 profile 测试里 hanvon 的期望 `:1720-1727`）
- `crates/library/src/generate.rs`（修改：记录迁移、删除保护、测试）
- `crates/library/src/deliver.rs`（修改：`put_file` 保护、测试）
- `crates/library/src/lib.rs` 或 `session.rs`（若迁移挂在连设备那一步）
- `crates/library/tests/flow.rs`（新增集成测试）
- `docs/devices.md`（`:358` 汉王传书一节）、`docs/usage.md`（`:337` 表格）、`docs/decisions.md`（新增一行）

## 约束
- 不做：不删、不改名、不写设备上的任何已有文件或目录（迁移只改记录）；不改 Kindle、掌阅、Bigme、Kobo 的 `dir`；不改产物内容；不升 `OPTIMIZE_VERSION` / `COMIC_VERSION`（如果你发现非升不可，停下写进「结果 → 遇到的问题」，状态改 blocked）；不读 KCC / Calibre 等 GPL 转换器源码
- 只在上面的分支工作；不合并；改「涉及文件」以外的文件写进「结果 → 偏离」
- 提交信息中文，`fix:` / `test:` / `docs:` 等前缀 + 正文，不加 Co-Authored-By；不读密钥、`.env`、`auth*`
- 测试一律用临时目录当设备（`BOOKLIB_MTP_NO_MOUNT_CHECK` / `common::open`），不碰真插着的设备
- 风险点：设备行为、数据迁移（生成记录）→ 要 agy 二审

## 规则（要实现的行为）
1. **汉王 `dir = "Documents"`**，注释写明：2026-11-11 小写 `documents` 让 MTP 多登记出一个同内容的 `documents`，改成设备上的真名；旧记录由规则 2 原样接过来。
2. **只差大小写的产物根目录 = 同一个位置（只改记录）**。设备接上（`Target::Dir { root }`）时，这台设备的生成记录里：`root` 和现在的 `root` 不相等、但逐段不分大小写相等的条目（`books` 的 `path`/`root`、各条 `old`、`orphans`），把 `root` 换成现在的，`path` 换成 `新 root + path 去掉旧 root 前缀`；**换之前看新路径上文件在**（`is_file()`），在才换（不分大小写的存储上一定在；大小写敏感的设备上新路径不在，就不换，照现在的挪位置流程走——那里两个目录真是两个，现有流程是对的）。不碰文件，记录有变才写一次。
   - 挂在哪：要保证 `prepare`/`prepare_file`、`space_check`、`flush_removed`、`retract` 看到的都是迁移后的记录。建议放在设备连接那一步（`Library::target` 第一次拿到 `Ok(Target::Dir)` 时，每轮最多一次），也可以在这几个入口各调一次；选哪种写进「结果」。
   - 迁移后同一本书走原来的"已是最新"（`generate.rs:733`），返回 `Built::UpToDate(新路径)`，不生成、不传、不读回设备上的文件。
3. **兜底保护：不分大小写相等、父目录不同的两个路径，绝不靠删一个来"挪"到另一个**（规则 2 之外的情况：用户把跟踪目录 `Manga` 改成 `manga`、规则 2 没换成的条目等）。
   - `deliver::put_file`：`src`、`dest` 逐段不分大小写相等（且不相等）、`keep_src = false` 时按 `keep_src = true` 处理：`same_bytes` 为真就返回 `Ok(false)`、**不删 src**；否则复制、不改名、不删 src。
   - `State::delete_placements`：要删的位置和本书当前位置（`current`）或别的记录里的位置逐段不分大小写相等的，不删文件、不删目录，记录丢掉（同现在 `distinct_on_disk == Some(false)` 的处理）。大小写敏感的设备上这会留下一个旧文件没删——宁可多留一份，不删书；在注释里写清楚。
   - 新加一个判断函数（如 `case_alias(a, b)`：`a != b` 且逐段 `to_lowercase` 相等），和现有的 `case_twin`（同一目录、只差文件名大小写）分开，**现有 `case_twin` 的行为不变**。
4. 文档：`docs/decisions.md` 加「2026-11-12 汉王产物目录改成 `Documents`；只差大小写的目录当同一个位置」；`docs/devices.md` 汉王传书一节、`docs/usage.md` 表格改成 `Documents/`，写一句掌阅 / Bigme 也是小写名、可能有同样的重影、没改。

## 分步任务
1. [ ] `generate.rs`：`case_alias` + `State` 上的迁移函数（如 `rebase_case_root(&mut self, new_root) -> bool`，返回改没改）。单元测试：
   - 旧 `root=…/documents`、文件在 `…/Documents/a/x.epub`（测试里 `Documents` 做成指向 `documents` 的符号链接，模拟不分大小写）：`path`、`root`、`old`、`orphans` 都换成 `Documents`，返回 true；再调一次返回 false（幂等）。
   - 新路径不在（两个真目录，模拟大小写敏感）：不换。
   - `root` 不是只差大小写（如 `books` → `Books2`、别的设备的路径）：不换。
2. [ ] 接入迁移（规则 2 的挂载点），自检：`cargo test -p library`。
3. [ ] 兜底保护：`deliver::put_file`、`State::delete_placements`（规则 3）。单元测试：符号链接模拟的同一目录下，`put_file(documents/x, Documents/x, keep_src=false, compare=true, direct=true)` 后文件还在、返回 `Ok(false)`；`delete_placements([documents/x], current=Documents/x)` 后文件还在、记录丢掉；现有 `case_twin` 相关测试（`generate.rs:1601` `case_only_rename_keeps_current`、`deliver.rs` 的 `Abc.epub`/`abc.epub`）照旧通过。
4. [ ] `hanvon.toml`、`profile/src/lib.rs` 测试期望。
5. [ ] 集成测试（`tests/flow.rs`）：`hanvon` 设备目录先用 `dir=documents` 生成两本（一本放子目录），再建符号链接 `Documents -> documents`、把 profile 的 `dir` 换成 `Documents`（克隆 profile 改 `deliver`，`refresh_devices()` 让连接重算），再同步：
   - 两本都是 `UpToDate`，路径在 `Documents/` 下；文件还在、内容和 inode / mtime 没变（没写）；`output-state/hanvon.json` 里路径都是 `Documents`；再同步一次仍 `UpToDate`。
   - 同一个测试不建符号链接（两个真目录）走一遍：书最后在 `Documents/` 下、能读到（照现有挪位置），不要求旧的删掉。
   - 一本书在 `documents/` 下有、同时把它从书库删掉（orphan）：迁移后 `flush_removed` 删的是 `Documents/` 下那份、只删这一本。
6. [ ] 文档（规则 4）。
7. [ ] 不回归：`cargo test --workspace` 全过；`fp_golden` 不重写就通过（`fp_golden.txt` 一行不变）；拿两本合成书在 master 和本分支分别 `epub-optimize --device=` 汉王 / Kindle / 掌阅 / Bigme / Kobo，产物逐字节相同（只改路径，应全部相同）。

## 验收标准
- [ ] 汉王产物根目录是 `<存储>/Documents`；旧记录（`documents`）在不分大小写的存储上原样接过来：不重新生成、不重传、不删、不改名设备上的任何文件
- [ ] 大小写敏感的设备上，只差大小写的目录变更不会因为"迁移"把记录指到不存在的文件
- [ ] 任何情况下，不分大小写相等的两个路径之间"挪位置"都不会删掉 src 或旧位置的文件（单元测试覆盖 `put_file` 和 `delete_placements`）
- [ ] Kindle、掌阅、Bigme、Kobo 的 `dir` 和产物逐字节不变；`fp_golden.txt` 不变；`OPTIMIZE_VERSION` 不变
- [ ] `cargo test --workspace` 全过；`cargo clippy --workspace --all-targets -- -D warnings` 0 警告；`cargo fmt --all -- --check` 干净
- [ ] 结果里写明没上真机。真机由 vinci 验（装好后汉王插上、挂好）：
  1. `booklib sync hanvon`：汉王的书全部显示已是最新 / 没有"生成""挪到"的行，用时和平时增量同步差不多；
  2. 汉王自带阅读器里书都在，挑两本看阅读进度还在；
  3. `output-state/hanvon.json` 里的路径都变成 `Documents`；
  4. 拔掉重插（或重启汉王）后，`ls /run/user/1000/mtp/hanvon/内部共享存储空间/` 里小写的 `documents` 不见了；还在的话**不要删**，记下现象。

## 交付物
代码提交到上面的分支并推送；「结果」填好；状态改为 done 或 blocked。

## 给执行方的指令
```
执行任务单 .dev/tasks/2026-11-12-hanvon-documents-case.md：先把状态改为 accepted 并提交，按「分步任务」做完，填好「结果」，状态改为 done（做不下去改为 blocked 并写明原因），提交并推送分支。
```

## 结果
<!-- 执行方填写 -->
- 执行方 / 开始–结束时间：opencode-local / 2026-11-12 09:16–09:40 +08:00
- base..head：`c3a1616..d94eec0`（`c3a1616` 是切出时的 master，含任务单提交；任务单写的 `73c095c` 是它前一个代码提交）
- 提交：

  | 步骤 | 提交号 | 文件 | 说明 |
  | --- | --- | --- | --- |
  | 接受 | `a47fe71` | 任务单 | 状态改 accepted |
  | 1–6 | `d94eec0` | `generate.rs`、`deliver.rs`、`flow.rs`、`hanvon.toml`、`profile/src/lib.rs`、`docs/*`、`README.md` | 迁移、兜底保护、测试、文档 |
  | 结果 | 本提交 | 任务单 | 填结果、状态改为 done |

- 实现：
  - **迁移**：`generate::case_alias(a,b)` = 逐段 `to_lowercase` 相等、且不是同一个路径；`State::rebase_case_root(new_root)` 把 `books[].path/root`、各 `old`、`orphans` 里根目录只差大小写、`new_root + 相对路径` 的文件 **`is_file()`** 的条目原样换到新根；返回改没改，幂等。
  - **挂载点**：放在生成入口 `prepare` / `space_check` / `flush_removed` / `retract` 的开头（`Library::rebase_state_case`），第一次拿到接上的 `Target::Dir` 后跑；先看记录里有没有只差大小写的根目录，有才克隆 / 写回，避免每本书都克隆全部记录。`list` / `outputs` 只读，不迁移、不写。
  - **兜底**：`deliver::put_file` 遇到 `case_alias(src,dest)` 且 `keep_src=false` 时按 `keep_src=true` 走：`same_bytes` 为真返回 `Ok(false)`、不删 src；否则复制、不改名、不删 src。`State::delete_placements` 遇到要删的位置和 `current` 或别的记录里的位置父目录不同、逐段只差大小写的，不删文件、不删目录，记录丢掉；同一目录只差文件名的大小写仍走原 `case_twin` + `distinct_on_disk`，行为不变。
  - **profile**：`hanvon.toml` `dir = "Documents"`，注释写 2026-11-11 现场和小写名导致 MTP 重影；`profile/src/lib.rs` 的期望改成 `Documents`。
  - **文档**：`docs/decisions.md` 新增 2026-11-12 一节；`docs/devices.md` 汉王传书、`docs/usage.md` 产地表和表下小注、`README.md` 设备表改成 `Documents/`；写清掌阅 / Bigme 也是小写名、可能有同样的重影、本次没改。
- 测试：

  | 命令 | 结果 | 摘要 |
  | --- | --- | --- |
  | `cargo test -p library` | 通过 | lib 74（含 `rebase_case_root_only_when_new_path_exists`、`case_alias_paths_are_not_deleted`、`put_file_case_alias_never_deletes_src`）、bin 7、cli 14、flow 51、fp 1 |
  | `cargo test --workspace` | 通过 | 全部通过（bookconv 553、bookbase 30、csscascade 16、kfx 53、library 74+7+14+51+1、profile 19） |
  | `cargo clippy --workspace --all-targets -- -D warnings` | 通过 | 0 警告 |
  | `cargo fmt --all -- --check` | 通过 | 干净 |
  | `git diff crates/library/tests/fp_golden.txt` | 未变 | `[deliver]` 不进指纹，金样一行没动 |
- 集成测试（`flow.rs`，都用临时目录 + 符号链接模拟不分大小写）：
  - `hanvon_documents_case_rebase_keeps_books_and_record`：旧 `documents` 生成 → 建 `Documents -> documents` → 改 profile `dir=Documents`、`refresh_devices()` → `Built::UpToDate`，路径在 `Documents/` 下；文件 inode / mtime 没变（没写）；`output-state/hanvon.json` 里是 `Documents`；再同步仍 `UpToDate`。
  - `hanvon_documents_case_sensitive_copies_and_keeps_old`：`Documents` 是空真目录（不建符号链接）→ 不迁移，走原来的挪位置；书最后在 `Documents/` 下能读，旧 `documents/` 文件保留（宁可多留、不删书）。
  - `hanvon_documents_case_migrates_orphans_too`：把记录从 `books` 挪进 `orphans`、建符号链接后 `flush_removed` 删的是 `Documents/` 下这一本，`documents` 旧路径跟着没了（同一个文件），记录清空。
- 不回归：合成 zh / en 两本在 master `c3a1616` 与本分支的 `epub-optimize` 上对 kindle / ireader / kobo / hanvon / bigme 各跑一遍，10 组产物逐字节相同；`OPTIMIZE_VERSION` / `COMIC_VERSION` 没动。
- 偏离：
  - 动了 `README.md`（设备表里汉王的 `documents/` → `Documents/`），不在「涉及文件」的点名列表里，但属于规则 4 的文档同步。
  - 迁移挂在生成入口（`prepare` / `space_check` / `flush_removed` / `retract`），不是 `Library::target`；理由：`list` 等只读命令也会拿 target，不能顺手写记录。已经在规则 2 允许的两种做法里选了后者。
  - 没有为 `library/src/lib.rs` / `session.rs` 加代码；迁移函数在 `generate.rs`。
- 未完成 / 待真机：
  - **没上真机**。真机由 vinci 验：装好后汉王插上、挂好：
    1. `booklib sync hanvon`：汉王的书全部显示已是最新 / 没有「生成」「挪到」的行，用时和平时增量同步差不多；
    2. 汉王自带阅读器里书都在，挑两本看阅读进度还在；
    3. `output-state/hanvon.json` 里的路径都变成 `Documents`；
    4. 拔掉重插（或重启汉王）后，`ls /run/user/1000/mtp/hanvon/内部共享存储空间/` 里小写的 `documents` 不见了；还在的话**不要删**，记下现象。
  - 掌阅 / Bigme 的小写 `dir` 没改，重影是否也存在没上真机确认；删除保护只是兜底。

### 返工（2026-11-12，一审退回后）

- 状态先改回 accepted（`184741e`），返工提交 `050ee1c`。
- **阻断 1（`put_file` 的 case_alias 分支 direct 照传）**：`deliver.rs` 的 `case_alias` 分支改成 `put_file_retry(src, dest, true, compare, false, mount)`，**强制不走直接写**：直接写会先删 dest，不分大小写时 dest 就是 src，书先没了、再从已不在的 src 复制失败。现在走「先写旁边临时文件、再删 dest、改名」那条路，临时文件写好之前不碰 dest。测试 `put_file_case_alias_never_deletes_src` 加了符号链接下 `compare=false, direct=true` 的用例：内容正确、src 还在、目录里没留临时文件；原来 `compare=true, direct=true` 的用例保留。
- **建议 1（吞掉写记录错误）**：`rebase_state_case` 改成 `Result<(), String>`；`prepare` / `retract` / `flush_removed` 用 `?` 报出来，`space_check` 返回 `SpaceCheck::Unknown("生成记录大小写迁移失败：…")`（只警告、照旧传）。
- **建议 2（每轮只迁移一次）**：`Devices` 加 `rebased: RefCell<HashSet<String>>` 和 `begin_rebase(id)`，`refresh()` 时清空；`rebase_state_case` 每台设备每轮只扫 / 迁移一次，避免一条换不成的记录让后面每本书都克隆整份记录、反复在 MTP 上 `is_file()`。
- **线索 1 核对**：`remove_outputs` / `hand_over_outputs` 不迁移；删书时按旧 `documents/...` 删的就是那本书（本来就要删），往上删空目录停在旧 `root`（`documents`），不会删 `Documents` 根目录本身。`delete_placements` 的新保护在大小写敏感设备上会留旧文件，任务单已接受「宁可多留」。Kobo / 掌阅「跟踪目录只改大小写」以前会删书、现在不删，符合本单兜底要求。
- 返工后测试：`cargo test -p library` 全过（lib 74、flow 51 等），`cargo test --workspace` 全过，`cargo clippy --workspace --all-targets -- -D warnings` 0 警告，`cargo fmt --all -- --check` 干净；`fp_golden.txt` 仍一行没动。
- 仍然**没上真机**；真机验证项同上。

## 审查
<!-- 负责 bot 填写 -->
- 验收（第 1 轮，sheng-ren，2026-11-12，box 复跑 `fce2a9a`）：**退回**，阻断 1 条，修完重新验收
  - 复跑：`cargo fmt --all -- --check` 干净；`cargo clippy --workspace --all-targets -- -D warnings` 0 警告；`cargo test --workspace` 全过（bookconv 553、kfx 53、bookbase 30、csscascade 16、library 74/7/14/51/1、profile 19，0 failed），和「结果」一致。
  - 复现写单时的"先删再重传"场景（临时集成测试，没提交）：`dir=documents` 生成一本 → `Documents -> documents` 符号链接 → 内置 hanvon（`Documents`）连续同步 3 次：每次都是 `UpToDate …/Documents/风起.epub`，书一直在，`output-state/hanvon.json` 路径已是 `Documents`。master 上同一场景第二次同步书被删、第三次重传。**主场景修好了。**
  - 逐条对照：
    - 规则 1：`hanvon.toml:42` `Documents` + 注释；`profile/src/lib.rs:1725` 期望已改。✓
    - 规则 2：迁移只在根目录只差大小写、且 `新 root + 相对路径` `is_file()` 时才换（`generate.rs:1483-1497`）；挂在 `prepare`/`space_check`/`flush_removed`/`retract` 开头（`generate.rs:639`、`:546`、`:481`、`:448`），偏离里写了理由，接受。flow 测试用符号链接真的模拟了不分大小写，并核对 inode / mtime 没变（`tests/flow.rs` `hanvon_documents_case_rebase_keeps_books_and_record`）；大小写敏感的两真目录不迁移（`:3212`）。✓
    - 规则 3：`delete_placements` 保护（`generate.rs:1245-1255`）✓；`put_file` 保护有漏洞，见阻断 1。
    - 不回归：`git diff master..HEAD -- crates/bookconv crates/library/tests/fp_golden.txt` 为空——产物代码和金样一行没动，`OPTIMIZE_VERSION` / `COMIC_VERSION` 没动，其他设备产物必然逐字节不变（执行方另跑了 10 组合成书对拍）。✓
    - 提交：中文前缀 + 正文，无 `Co-Authored-By`，没动 master。`README.md` 偏离已报，接受。
  - **阻断 1：`deliver.rs:209` 的大小写保护只把 `keep_src` 改成 true，`direct` 照传；`put_file_once` 直接写时先删 dest（`deliver.rs:391-392`），不分大小写的设备上 dest 就是 src，书先被删、再从已不在的 src 复制失败。** 走到这里的条件：`compare = false`，或 `same_bytes`（`deliver.rs:528`，任何读错都返回 false）在 MTP 上读失败一次。已复现：符号链接 `Documents -> documents`，`put_file(documents/书.epub, Documents/书.epub, keep_src=false, compare=false, direct=true)` → `Err("写 …/Documents/书.epub: No such file or directory")`，`documents/书.epub` 没了。现在 `prepare_file` 挪位置时 `compare` 恒为 true，平时走不到，但这正是任务单「任何情况下都不会删掉 src」要兜住的情况。
    - 要改：`case_alias` 分支改成 `put_file_retry(src, dest, true, compare, false, mount)`，即**强制不直接写**，走"先写旁边的临时文件、再删 dest、改名"那条路（临时文件写好之前不碰 dest；不分大小写的设备上删 dest 时新内容已经在临时文件里）。注释写清为什么不能直接写。
    - 测试：`put_file_case_alias_never_deletes_src`（`deliver.rs:817`）加两种：符号链接下 `compare=false, direct=true`；符号链接下 `compare=true, direct=true` 但内容不同（如先把 dest 那一侧看成不同——可用 `FAIL_WRITE` 之外的办法，做不到就只测 `compare=false`）。都要求调用后文件在、内容是 src 的内容、目录里没留临时文件。
  - 建议（这轮一起改，不改的在「结果」说明理由）：
    1. `generate.rs:310` `let _ = self.records.states.put(...)` 吞掉了写记录的错误：迁移没写成，后面按新根目录算位置，旧记录照旧，就回到挪位置流程（有了保护不会删书，但会整本"挪"一遍）。改成返回 `Result`，在 `prepare` 等入口用 `?` 报出来（`retract`/`flush_removed` 同样）。
    2. `generate.rs:295-312` 每次 `prepare` 都扫一遍全部记录；有一条换不成（设备上那本被手工删了、半截记录）时 `maybe` 一直为真，每本书都克隆整份记录、再对换不成的那几条 `is_file()`（MTP 上每次 stat 都是一次往返）。同步一轮后会自愈，但第一轮是 k×n 次 stat。建议每台设备每轮只迁移一次（在 `Devices` 里记一个本轮已迁移的标记，`refresh` 时清掉）。
  - 线索（留给二审）：
    1. `remove_outputs`、`hand_over_outputs`（`generate.rs:411` 起、`:429` 起）不迁移：删书时按旧的小写路径删，在汉王上删的就是那本书本身（本来就要删），往上删空目录停在旧 `root`（重影的 `documents`），请核实不会删到 `Documents` 本身。
    2. `delete_placements` 新保护（`generate.rs:1247-1254`）也比对别的书的记录：大小写敏感的设备上会留下旧文件不删，任务单已接受"宁可多留"。
    3. 这条保护也改变了 Kobo（vfat）/ 掌阅上"跟踪目录只改大小写"（`Manga` → `manga`）的行为：以前会删书，现在不删、记录跟过去。请核实没有别的现有流程依赖旧行为。
    4. 汉王自带阅读器的进度是不是按路径记的未知，真机验。
- 二审：
- 合并：
