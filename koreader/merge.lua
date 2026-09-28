-- merge.lua —— KOReader 配置文件深合并（LuaJIT / Lua 5.1 语法；电脑上用 luajit 跑）。
-- 用法: luajit merge.lua <目标.lua> <补丁.lua> [--dry-run]
--   目标/补丁都是 `return { ... }` 形式（settings.reader.lua、settings/gestures.lua、settings/profiles.lua 皆如此）。
--   语义：补丁里的标量覆盖；两边都是表则递归合并；补丁里值为字符串 "__DELETE__" 则删键。
--   输出：每处改动一行 `路径: 旧值 → 新值`；--dry-run 只算差异不写。写入 = 先写 .tmp 再改名。
--   退出码：0 = 有改动，10 = 没有改动，其它 = 出错。
package.path = (arg[0]:match("^(.*)/[^/]*$") or ".") .. "/?.lua;" .. package.path
local L = require("luaser")

local target_path, patch_path, flag = arg[1], arg[2], arg[3]
if not target_path or not patch_path then
  io.stderr:write("用法: merge.lua <目标.lua> <补丁.lua> [--dry-run]\n"); os.exit(2)
end

local changes = {}
-- 补丁里的表整体落到目标（目标原先没有这张表，或原先不是表）时：深拷贝并剔除 "__DELETE__" 标记，
-- 否则删除标记会被当成普通字符串写进配置。
local function copy_without_deletes(v)
  if type(v) ~= "table" then return v end
  local out = {}
  for k, x in pairs(v) do
    if x ~= L.DELETE then out[k] = copy_without_deletes(x) end
  end
  return out
end
local function merge(dst, src, path)
  for k, v in pairs(src) do
    local p = (path == "" and tostring(k)) or (path .. "." .. tostring(k))
    if v == L.DELETE then
      if dst[k] ~= nil then changes[#changes + 1] = { p, dst[k], nil }; dst[k] = nil end
    elseif type(v) == "table" and type(dst[k]) == "table" then
      merge(dst[k], v, p)
    else
      local nv = copy_without_deletes(v)
      if not L.equal(dst[k], nv) then
        changes[#changes + 1] = { p, dst[k], nv }
        dst[k] = nv
      end
    end
  end
end

local target = L.load(target_path, false)
merge(target, L.load(patch_path, true), "")
table.sort(changes, function(a, b) return a[1] < b[1] end)
for _, c in ipairs(changes) do
  io.write(c[1], ": ", L.show(c[2]), " → ", L.show(c[3]), "\n")
end
if #changes == 0 then os.exit(10) end
if flag ~= "--dry-run" then L.write(target_path, target) end
