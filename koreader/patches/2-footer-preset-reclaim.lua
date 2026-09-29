-- 状态栏预设连"状态栏覆盖正文"（footer.reclaim_height）一起切（用户 2026-09-29：漫画四边尽量只留 1px）。由 koreader/apply.sh 拷到设备的
-- koreader/patches/；不想要了删掉设备上的这个文件即可。
--
-- 漫画方案靠「漫画」状态栏预设隐藏状态栏，但隐藏了 KOReader 仍按状态栏高度留出页面底部（约 40px），除非 reclaim_height 开着；
-- 全局开着又不行：文字书状态栏一直显示，开了它状态栏会压住正文最后一行（readertypeset.lua onSetPageMargins）。
-- 所以只在「漫画」预设里开（presets.lua 生成）。但 ReaderFooter:loadPreset 只把预设的 footer 表存进设置，没更新它缓存的
-- self.reclaim_height，也不重算页边距（2026-09-29 本机 KOReader v2026.07.1 实测：截图底部照样空 39px）。这里补上这两步，
-- 做法同状态栏菜单里切换这个开关（readerfooter.lua：更新 self.reclaim_height 后 refreshFooter(true, true) 重发页边距）。
local ReaderFooter = require("apps/reader/modules/readerfooter")

local orig_loadPreset = ReaderFooter.loadPreset
ReaderFooter.loadPreset = function(self, preset)
    local prev = self.reclaim_height
    orig_loadPreset(self, preset)
    self.reclaim_height = self.settings.reclaim_height
    if self.reclaim_height ~= prev then
        self:refreshFooter(true, true)
    end
end
