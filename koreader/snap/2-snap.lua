-- snap.sh 用的 KOReader 用户补丁（放进隔离的 KO_HOME/patches/）：书打开后逐页截图，写到 $KOSNAP_OUT，截完退出。
-- 每页一张 pNNN.png，info.txt 里记总页数和每页开头的 xpointer（DocFragment[n] = 第 n 个 spine 文件）。
local out = os.getenv("KOSNAP_OUT")
if not out then return end
local max = tonumber(os.getenv("KOSNAP_PAGES")) or 20
local ReaderUI = require("apps/reader/readerui")
local UIManager = require("ui/uimanager")
local Event = require("ui/event")
local Screen = require("device").screen

local orig_init = ReaderUI.init
ReaderUI.init = function(self, ...)
    orig_init(self, ...)
    -- 等开书时的提示框（"书籍信息缓存已更新"之类）自己消失再截
    UIManager:scheduleIn(5, function()
        local total = self.document:getPageCount()
        local f = assert(io.open(out .. "/info.txt", "w"))
        f:write("pages=", total, "\n")
        for i = 1, math.min(max, total) do
            self:handleEvent(Event:new("GotoPage", i))
            UIManager:forceRePaint()
            Screen:shot(string.format("%s/p%03d.png", out, i))
            f:write(i, " ", tostring(self.document.getXPointer and self.document:getXPointer() or ""), "\n")
        end
        f:close()
        UIManager:quit()
    end)
end
