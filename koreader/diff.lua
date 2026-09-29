-- diff.lua —— 两份 KOReader 配置表的净差异（逐个叶子键比较）。
-- 用法: luajit diff.lua <原文件.lua> <新文件.lua>   输出每处不同一行 `路径: 旧值 → 新值`；退出码 0 = 有不同，10 = 完全相同。
-- apply.sh / check.sh 用它判断"最后要不要写"：补丁是分层覆盖的（个人设置 → 方案 → 设备），中间层改过、后面又改回来的键
-- 不算改动，只看原文件和最终结果。
package.path = (arg[0]:match("^(.*)/[^/]*$") or ".") .. "/?.lua;" .. package.path
local L = require("luaser")

local out = L.diff(L.load(arg[1], false), L.load(arg[2], true))
for _, l in ipairs(out) do io.write(l, "\n") end
os.exit(#out > 0 and 0 or 10)
