-- presets.lua —— 按设备当前的状态栏生成「文字」「漫画」两个状态栏预设，写进 settings.reader.lua 的 footer_presets。
-- 用法: luajit presets.lua <settings.reader.lua> [--dry-run]   输出与退出码同 merge.lua（0 有改动，10 没改动）。
--
-- 为什么生成而不是写死：载入预设会把整张状态栏设置（footer）连同字体文件路径一起换掉（源码 ReaderFooter:loadPreset），
-- 而字体路径随设备不同；写死的预设过一阵还会跟设备上手改的状态栏对不上。所以每次 apply 都从设备现在的状态栏生成：
--   「文字」= 现在的状态栏（结构同源码 ReaderFooter:buildPreset）；
--   「漫画」= 同一套设置，但隐藏（reader_footer_mode = 0）且不显示进度条。
-- 正读着漫画时状态栏是隐藏的（mode 0），这时不重新生成「文字」，保留原来的。
package.path = (arg[0]:match("^(.*)/[^/]*$") or ".") .. "/?.lua;" .. package.path
local L = require("luaser")

local path, flag = arg[1], arg[2]
local s = L.load(path, true)
local presets = L.copy(s.footer_presets or {})
local mode = s.reader_footer_mode or 1
if s.footer and mode ~= 0 then
  presets["文字"] = {
    footer = L.copy(s.footer),
    reader_footer_mode = mode,
    reader_footer_custom_text = s.reader_footer_custom_text or "KOReader",
    reader_footer_custom_text_repetitions = s.reader_footer_custom_text_repetitions or "1",
  }
end
if type(presets["文字"]) == "table" and type(presets["文字"].footer) == "table" then
  local comic = L.copy(presets["文字"])
  comic.footer.disable_progress_bar = true
  -- 隐藏的状态栏不再占页面底部（要配 patches/2-footer-preset-reclaim.lua：KOReader 载入预设时自己不切这个开关）
  comic.footer.reclaim_height = true
  comic.reader_footer_mode = 0
  presets["漫画"] = comic
end

local changed = 0
for _, name in ipairs({ "文字", "漫画" }) do
  local old = (s.footer_presets or {})[name]
  if not L.equal(old, presets[name]) then
    changed = changed + 1
    io.write("footer_presets.", name, ": ", old and "更新" or "新建", "（按当前状态栏生成）\n")
  end
end
if changed == 0 then os.exit(10) end
if flag ~= "--dry-run" then
  s.footer_presets = presets
  L.write(path, s)
end
