-- snap.sh 用的 KOReader 用户补丁（放进隔离的 KO_HOME/patches/）：书打开后逐页处理，写到 $KOSNAP_OUT，处理完退出。
--   截图（缺省）：每页一张 pNNN.png；info.txt 里记总页数和每页开头的 xpointer（DocFragment[n] = 第 n 个 spine 文件）。
--   KOSNAP_LINKS=1：不截图，改为检查每页上的书内链接会不会按注释弹窗——调用 KOReader 自己的判定
--   （ReaderLink:showAsFootnotePopup，设置里的 footnote_link_in_popup / link_prefer_footnote 照样生效），
--   弹窗不真的显示；结果写 links.txt：「页 弹窗|跳转 链接文字 目标」，最后一行是合计。
local out = os.getenv("KOSNAP_OUT")
if not out then return end
local max = tonumber(os.getenv("KOSNAP_PAGES")) or 20
local links_mode = os.getenv("KOSNAP_LINKS") == "1"
local ReaderUI = require("apps/reader/readerui")
local UIManager = require("ui/uimanager")
local Event = require("ui/event")
local Screen = require("device").screen

-- 一页上的书内链接：问 KOReader 是不是注释（弹窗），是就记"弹窗"，不是记"跳转"
local function probe_links(ui, page, f, total)
    local links = ui.document:getPageLinks(true) or {}
    for _, l in ipairs(links) do
        if l.section and l.a_xpointer then
            local shown = false
            local orig_show = UIManager.show
            UIManager.show = function(_, widget) shown = true; if widget and widget.free then pcall(widget.free, widget) end end
            local ok, popup = pcall(ui.link.showAsFootnotePopup, ui.link, { xpointer = l.section, from_xpointer = l.a_xpointer, a_xpointer = l.a_xpointer }, true)
            UIManager.show = orig_show
            popup = ok and popup and shown
            local text = (ui.document:getTextFromXPointer(l.a_xpointer) or ""):gsub("%s+", " ")
            f:write(page, " ", popup and "弹窗" or "跳转", " ", text:sub(1, 30), " ", l.section, "\n")
            total[popup and "popup" or "jump"] = total[popup and "popup" or "jump"] + 1
        end
    end
end

local orig_init = ReaderUI.init
ReaderUI.init = function(self, ...)
    orig_init(self, ...)
    -- 等开书时的提示框（"书籍信息缓存已更新"之类）自己消失再处理
    UIManager:scheduleIn(links_mode and 1 or 5, function()
        local total = self.document:getPageCount()
        local f = assert(io.open(out .. (links_mode and "/links.txt" or "/info.txt"), "w"))
        if not links_mode then f:write("pages=", total, "\n") end
        local count = { popup = 0, jump = 0 }
        for i = 1, math.min(max, total) do
            self:handleEvent(Event:new("GotoPage", i))
            if links_mode then
                probe_links(self, i, f, count)
            else
                UIManager:forceRePaint()
                Screen:shot(string.format("%s/p%03d.png", out, i))
                f:write(i, " ", tostring(self.document.getXPointer and self.document:getXPointer() or ""), "\n")
            end
        end
        if links_mode then f:write("合计 弹窗 ", count.popup, " 跳转 ", count.jump, "\n") end
        f:close()
        UIManager:quit()
    end)
end
