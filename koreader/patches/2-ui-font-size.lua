-- 界面默认字号整体小 2 号（用户 2026-09-28：所有设置项字体小 2 号）。由 koreader/apply.sh 拷到设备的 koreader/patches/；
-- 不想要了删掉设备上的这个文件即可（KOReader 菜单里也能「停用所有用户补丁」）。
--
-- 界面各处的缺省字号写死在 frontend/ui/font.lua 的 Font.sizemap（设置菜单的每一项用 smallinfofont 22、菜单底栏 ffont 20、
-- 标题 tfont 26、提示 infofont 24……），KOReader 没有覆盖它的设置（界面字体有 fontmap，字号没有）。
-- 文件名 2- 开头 = 界面管理器加载之后、文件管理器和阅读界面打开之前执行（reader.lua），这时还没有菜单，改表对全部界面生效。
-- 自己指定字号的列表不在这张表里、不受影响：文件列表、目录、书签（各自菜单里的「字号」）、键盘。
local Font = require("ui/font")

local DELTA = -2
local MIN = 10
for name, size in pairs(Font.sizemap) do
    Font.sizemap[name] = math.max(MIN, size + DELTA)
end
