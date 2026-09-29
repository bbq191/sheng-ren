-- luaser.lua —— merge.lua、presets.lua、diff.lua、unmerge.lua 共用：读 `return { … }` 形式的 KOReader 配置文件、
-- 按 KOReader 的风格写回，以及合并语义本身。
local M = {}

M.DELETE = "__DELETE__"

--- 读配置表。文件不存在：`must` 时报错退出，否则当空表。
--- 配置文件在空环境里执行（setfenv）：它本该只是一张表，不该能调用 os/io。
function M.load(path, must)
  local f = io.open(path, "r")
  if not f then
    if must then io.stderr:write("读不到 " .. path .. "\n"); os.exit(2) end
    return {}
  end
  local src = f:read("*a"); f:close()
  local chunk, err = loadstring(src, "=" .. path)
  if not chunk then io.stderr:write("解析失败 " .. path .. ": " .. tostring(err) .. "\n"); os.exit(3) end
  setfenv(chunk, {})
  local ok, t = pcall(chunk)
  if not ok or type(t) ~= "table" then io.stderr:write(path .. " 不是 return {…}\n"); os.exit(3) end
  return t
end

--- 数字写成 Lua 能原样读回的字面量：±inf、nan 没有字面量，用表达式；超出 2^53 的整数不能用 %d（会溢出成负数）。
local function number_literal(v)
  if v ~= v then return "0/0" end
  if v == math.huge then return "math.huge" end
  if v == -math.huge then return "-math.huge" end
  if v % 1 == 0 and math.abs(v) < 2 ^ 53 then return string.format("%d", v) end
  return string.format("%.17g", v)
end

local function lkey(k)
  if type(k) == "string" then return "[" .. string.format("%q", k) .. "]" end
  if type(k) == "number" then return "[" .. number_literal(k) .. "]" end
  return "[" .. tostring(k) .. "]"
end

-- 键排序：数字 < 字符串 < 布尔 < 其它；同类型数字、字符串按值，其余按 tostring（布尔之间不能直接比较）。
local type_rank = { number = 1, string = 2, boolean = 3 }
local function key_less(a, b)
  local ta, tb = type(a), type(b)
  if ta ~= tb then return (type_rank[ta] or 4) < (type_rank[tb] or 4) end
  if ta == "number" or ta == "string" then return a < b end
  return tostring(a) < tostring(b)
end

--- 序列化（KOReader 风格：键排序、数字键在前、字符串 %q、缩进 4 空格）。
function M.serialize(v, indent)
  indent = indent or 0
  local t = type(v)
  if t == "string" then return string.format("%q", v) end
  if t == "number" then return number_literal(v) end
  if t == "boolean" then return tostring(v) end
  if t == "table" then
    local keys = {}
    for k in pairs(v) do keys[#keys + 1] = k end
    table.sort(keys, key_less)
    if #keys == 0 then return "{}" end
    local pad = string.rep("    ", indent + 1)
    local out = { "{" }
    for _, k in ipairs(keys) do
      out[#out + 1] = pad .. lkey(k) .. " = " .. M.serialize(v[k], indent + 1) .. ","
    end
    out[#out + 1] = string.rep("    ", indent) .. "}"
    return table.concat(out, "\n")
  end
  return "nil"
end

--- 原子写回（先写 .tmp 再改名）。
function M.write(path, tbl)
  local tmp = path .. ".tmp"
  local f = assert(io.open(tmp, "w"))
  f:write("-- we can read Lua syntax here!\nreturn ", M.serialize(tbl), "\n")
  f:close()
  assert(os.rename(tmp, path))
end

--- 改动列表里显示的值：压成一行，太长截断。
function M.show(v)
  if v == nil then return "（无）" end
  local s = M.serialize(v):gsub("%s*\n%s*", " ")
  if #s > 120 then s = s:sub(1, 117) .. "…" end
  return s
end

function M.copy(v)
  if type(v) ~= "table" then return v end
  local t = {} for k, x in pairs(v) do t[k] = M.copy(x) end return t
end

function M.equal(a, b)
  if type(a) ~= type(b) then return false end
  if type(a) ~= "table" then return a == b end
  for k, x in pairs(a) do if not M.equal(x, b[k]) then return false end end
  for k in pairs(b) do if a[k] == nil then return false end end
  return true
end

--- 纯序列（键正好是 1..n，n ≥ 1），比如页边距 { 10, 10 }：合并时整体替换，不按下标递归。
function M.is_seq(v)
  if type(v) ~= "table" then return false end
  local n = 0
  for _ in pairs(v) do n = n + 1 end
  return n > 0 and n == #v
end

--- 补丁里的表整体落到目标时：深拷贝并剔除 "__DELETE__" 标记（否则删除标记会被当成普通字符串写进配置）。
local function copy_without_deletes(v)
  if type(v) ~= "table" then return v end
  local out = {}
  for k, x in pairs(v) do
    if x ~= M.DELETE then out[k] = copy_without_deletes(x) end
  end
  return out
end

local function join(path, k)
  return (path == "" and tostring(k)) or (path .. "." .. tostring(k))
end

--- 深合并：补丁里的标量覆盖；两边都是（非序列的）表则递归；值为 "__DELETE__" 则删键；序列整体替换。
--- 每处改动追加到 changes（{ 路径, 旧值, 新值 }），changes 可以为 nil。
function M.merge(dst, src, changes, path)
  path = path or ""
  for k, v in pairs(src) do
    local p = join(path, k)
    if v == M.DELETE then
      if dst[k] ~= nil then
        if changes then changes[#changes + 1] = { p, dst[k], nil } end
        dst[k] = nil
      end
    elseif type(v) == "table" and type(dst[k]) == "table" and not M.is_seq(v) then
      M.merge(dst[k], v, changes, p)
    else
      local nv = copy_without_deletes(v)
      if not M.equal(dst[k], nv) then
        if changes then changes[#changes + 1] = { p, dst[k], nv } end
        dst[k] = nv
      end
    end
  end
end

--- 补丁动到的每个位置（键路径数组）：合并时整体覆盖或删除的那一层为止。卸载时按这些位置逐个还原。
function M.leaf_paths(patch, prefix, out)
  prefix, out = prefix or {}, out or {}
  for k, v in pairs(patch) do
    local p = { unpack(prefix) }
    p[#p + 1] = k
    if type(v) == "table" and not M.is_seq(v) and next(v) ~= nil then
      M.leaf_paths(v, p, out)
    else
      out[#out + 1] = p
    end
  end
  return out
end

function M.get(t, path)
  for _, k in ipairs(path) do
    if type(t) ~= "table" then return nil end
    t = t[k]
  end
  return t
end

--- 按路径设值（v 为 nil 即删除），中间缺的表自动建；删除后沿路径向上清掉变空的表。
function M.set(t, path, v)
  local chain = { t }
  for i = 1, #path - 1 do
    local k = path[i]
    if type(chain[i][k]) ~= "table" then
      if v == nil then return end
      chain[i][k] = {}
    end
    chain[i + 1] = chain[i][k]
  end
  chain[#path][path[#path]] = M.copy(v)
  if v == nil then
    for i = #path - 1, 1, -1 do
      if next(chain[i + 1]) ~= nil then break end
      chain[i][path[i]] = nil
    end
  end
end

--- 两张配置表的净差异（逐个叶子键比较），每处一行 `路径: 旧值 → 新值`，已排序。
function M.diff(a, b)
  local out = {}
  local function walk(x, y, path)
    local keys = {}
    if type(x) == "table" then for k in pairs(x) do keys[k] = true end end
    if type(y) == "table" then for k in pairs(y) do keys[k] = true end end
    for k in pairs(keys) do
      local p = join(path, k)
      local vx, vy = x and x[k], y and y[k]
      if type(vx) == "table" and type(vy) == "table" then
        walk(vx, vy, p)
      elseif not M.equal(vx, vy) then
        out[#out + 1] = p .. ": " .. M.show(vx) .. " → " .. M.show(vy)
      end
    end
  end
  walk(a, b, "")
  table.sort(out)
  return out
end

return M
