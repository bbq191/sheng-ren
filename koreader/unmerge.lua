-- unmerge.lua —— 撤销 apply.sh 写进一个配置文件的方案/设备设置（apply.sh --uninstall 用）。
-- 用法: luajit unmerge.lua <目标.lua> <原始配置.lua 或 -> <层>… [--dry-run]
--   层按 apply 时的顺序给出：`+补丁.lua` = 保留的层（个人设置），`-补丁.lua` = 要撤销的层（文字书方案、设备）。
--   原始配置：第一次 apply 之前设备上的那份（最早的备份）；没有就给 -，撤销的键恢复成 KOReader 缺省（即删掉）。
-- 规则：对要撤销的层动过的每个位置，设备上现在的值如果还是 apply 写进去的值，就还原成「原始配置 + 保留层」的值；
--   现在的值跟 apply 写的不一样（之后在设备上手改过），不动。
-- 输出、退出码同 merge.lua（0 有改动，10 没有改动）。
package.path = (arg[0]:match("^(.*)/[^/]*$") or ".") .. "/?.lua;" .. package.path
local L = require("luaser")

local target_path, pristine_path = arg[1], arg[2]
if not target_path or not pristine_path then
  io.stderr:write("用法: unmerge.lua <目标.lua> <原始配置.lua|-> [+保留层.lua|-撤销层.lua]… [--dry-run]\n"); os.exit(2)
end
local layers, dry = {}, false
for i = 3, #arg do
  if arg[i] == "--dry-run" then dry = true else layers[#layers + 1] = arg[i] end
end

local cur = L.load(target_path, false)
local before = L.copy(cur)
local applied = L.copy(cur)       -- apply 会把设备现在的配置变成什么
local baseline = pristine_path == "-" and {} or L.load(pristine_path, false) -- 撤销后应该是什么（保留层也在内）
local revert = {}                 -- 要撤销的位置
for _, layer in ipairs(layers) do
  local sign, file = layer:sub(1, 1), layer:sub(2)
  local patch = L.load(file, true)
  L.merge(applied, patch)
  if sign == "+" then
    L.merge(baseline, patch)
  elseif sign == "-" then
    for _, p in ipairs(L.leaf_paths(patch)) do revert[#revert + 1] = { path = p } end
  else
    io.stderr:write("层要以 + 或 - 开头: " .. layer .. "\n"); os.exit(2)
  end
end

for _, r in ipairs(revert) do
  if L.equal(L.get(cur, r.path), L.get(applied, r.path)) then
    L.set(cur, r.path, L.get(baseline, r.path))
  end
end

local out = L.diff(before, cur)
for _, l in ipairs(out) do io.write(l, "\n") end
if #out == 0 then os.exit(10) end
if not dry then L.write(target_path, cur) end
