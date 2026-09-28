-- luaser.lua —— merge.lua、presets.lua 共用：读 `return { … }` 形式的 KOReader 配置文件、按 KOReader 的风格写回。
local M = {}

M.DELETE = "__DELETE__"

--- 读配置表。文件不存在：`must` 时报错退出，否则当空表。
function M.load(path, must)
  local f = io.open(path, "r")
  if not f then
    if must then io.stderr:write("读不到 " .. path .. "\n"); os.exit(2) end
    return {}
  end
  local src = f:read("*a"); f:close()
  local chunk, err = loadstring(src, "=" .. path)
  if not chunk then io.stderr:write("解析失败 " .. path .. ": " .. tostring(err) .. "\n"); os.exit(3) end
  local ok, t = pcall(chunk)
  if not ok or type(t) ~= "table" then io.stderr:write(path .. " 不是 return {…}\n"); os.exit(3) end
  return t
end

local function lkey(k)
  if type(k) == "string" then return "[" .. string.format("%q", k) .. "]" end
  return "[" .. tostring(k) .. "]"
end

--- 序列化（KOReader 风格：键排序、数字键在前、字符串 %q、缩进 4 空格）。
function M.serialize(v, indent)
  indent = indent or 0
  local t = type(v)
  if t == "string" then return string.format("%q", v) end
  if t == "number" then return (v % 1 == 0) and string.format("%d", v) or string.format("%.17g", v) end
  if t == "boolean" then return tostring(v) end
  if t == "table" then
    local keys = {}
    for k in pairs(v) do keys[#keys + 1] = k end
    table.sort(keys, function(a, b)
      local ta, tb = type(a), type(b)
      if ta ~= tb then return ta == "number" end
      return a < b
    end)
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

--- 让同目录的模块能被 require（脚本从任意工作目录调用都行）。
function M.dir_of(script)
  return (script or ""):match("^(.*)/[^/]*$") or "."
end

return M
