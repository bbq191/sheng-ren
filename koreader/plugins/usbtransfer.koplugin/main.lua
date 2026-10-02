-- USB 传书（sheng-ren 的 koreader/plugins/usbtransfer.koplugin，用户 2026-10-02 要）：Kindle 独占开机时，KOReader 开着不能插线拷书
-- （它的程序文件在书库分区上，电脑接管时会崩），原来只能「退出 → 整机重启 → 自带界面里插线」。
-- 这里点一下：留个记号 /tmp/koreader-boot.usb，再像菜单里的「退出」一样关掉 KOReader（会存进度）；开机自启的 run.sh
-- （koreader/kindle-boot/）看到记号就不重启、开 MTP（bin/usb.sh），拔线后再把 KOReader 起回来。
-- 只在 Kindle、KOReader 是 run.sh 起的（环境变量 KOREADER_BOOT=1）、有 configfs 的 USB gadget 时启用；别的情况下自己隐藏
-- （没人接着开 MTP、再起 KOReader，点了就只是退出）。

local Device = require("device")
local lfs = require("libs/libkoreader-lfs")

local GADGET = "/sys/kernel/config/usb_gadget/mtpgadget"
local FLAG = "/tmp/koreader-boot.usb"

if not (Device:isKindle() and os.getenv("KOREADER_BOOT") == "1" and lfs.attributes(GADGET, "mode") == "directory") then
    return { disabled = true }
end

local ConfirmBox = require("ui/widget/confirmbox")
local Dispatcher = require("dispatcher")
local UIManager = require("ui/uimanager")
local WidgetContainer = require("ui/widget/container/widgetcontainer")
local logger = require("logger")
local _ = require("gettext")

local UsbTransfer = WidgetContainer:extend{
    name = "usbtransfer",
}

function UsbTransfer:init()
    self:onDispatcherRegisterActions()
    self.ui.menu:registerToMainMenu(self)
end

function UsbTransfer:onDispatcherRegisterActions()
    Dispatcher:registerAction("usbtransfer", { category = "none", event = "UsbTransfer", title = _("USB 传书"), device = true })
end

function UsbTransfer:start()
    local f = io.open(FLAG, "w")
    if not f then
        logger.warn("UsbTransfer: 写不了", FLAG)
        return
    end
    f:close()
    -- 和菜单里的「退出」一样：关掉当前的书或文件管理器（存进度），界面全关了 KOReader 就退出
    self.ui.menu:exitOrRestart()
end

function UsbTransfer:onUsbTransfer()
    UIManager:show(ConfirmBox:new{
        text = _("退出 KOReader，打开 USB 传书？\n插上线就能在电脑上拷书，拔线后自动回到 KOReader。"),
        ok_text = _("USB 传书"),
        ok_callback = function() self:start() end,
    })
    return true
end

function UsbTransfer:addToMainMenu(menu_items)
    menu_items.usbtransfer = {
        text = _("USB 传书"),
        sorting_hint = "tools",
        callback = function() self:onUsbTransfer() end,
    }
end

return UsbTransfer
