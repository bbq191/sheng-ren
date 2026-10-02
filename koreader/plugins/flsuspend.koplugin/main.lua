-- 休眠关前光（sheng-ren 的 koreader/plugins/flsuspend.koplugin，用户 2026-10-02 要）：
-- KOReader 在 Kindle 上自己不关前光——KindlePowerD:beforeSuspend 不动灯，afterResume 注释写明"靠亚马逊界面自动开关前光"。
-- 平时界面在跑，休眠时界面把灯关了；独占（koreader.sh --framework_stop 停掉 lab126_gui）后没人关，休眠时前光一直亮着（2026-10-02 真机）。
--
-- 关灯不能只调 turnOffFrontlightHW 就指望系统唤醒时自己开回来：KindlePowerD:setIntensityHW 结尾会 _decideFrontlightState()，
-- 按硬件当前亮度重判"灯开没开"，把灯调到 0 就把"开着"的逻辑状态也翻成"关着"；于是唤醒时 afterResume 和自动前光插件都当灯本来就关、谁都不开（2026-10-02 真机：唤醒灯回不来）。
-- 所以这里休眠时先记住亮度再关，唤醒时自己按记下的亮度恢复（setIntensity 会把逻辑状态设回"开着"）。自动前光插件开着的话，它随后再按光线调，不冲突。
-- 只在 Kindle、框架被停（STOP_FRAMEWORK=yes，koreader.sh 导出）、有前光时启用。

local Device = require("device")

if not (Device:isKindle() and Device:hasFrontlight() and os.getenv("STOP_FRAMEWORK") == "yes") then
    return { disabled = true }
end

local UIManager = require("ui/uimanager")
local WidgetContainer = require("ui/widget/container/widgetcontainer")
local logger = require("logger")

local FlSuspend = WidgetContainer:extend{
    name = "flsuspend",
}

local function powerd()
    local ok, p = pcall(Device.getPowerDevice, Device)
    return ok and p or nil
end

function FlSuspend:onSuspend()
    local p = powerd()
    if p and p:isFrontlightOn() then
        self.saved = p:frontlightIntensity() -- 记住当前亮度，唤醒时恢复
        p:turnOffFrontlightHW()
        logger.dbg("FlSuspend: 休眠，关前光，记下亮度", self.saved)
    else
        self.saved = nil
    end
end

function FlSuspend:onResume()
    local p = powerd()
    if p and self.saved then
        local intensity = self.saved
        self.saved = nil
        -- 像 KindlePowerD:afterResume 那样放到下一拍再设，避开和系统的竞争
        UIManager:tickAfterNext(function()
            p:setIntensity(intensity) -- 公开接口：设硬件 + 把逻辑状态设回"开着"
            logger.dbg("FlSuspend: 唤醒，恢复前光到", intensity)
        end)
    end
end

return FlSuspend
