-- 自动前光（sheng-ren 的 koreader/plugins/autolight.koplugin，用户 2026-10-02 要）：KOReader 自己读 Kindle 的光线传感器、自己调前光。
--
-- 为什么不靠 Kindle 自带的「自动亮度」：开机独占时亚马逊界面被停掉，自带的自动调光不跑；KOReader 也会在唤醒时
-- 把亮度设回它记着的值。光照直接读传感器芯片（见 find_iio_lux：独占时 powerd 的 alsLux 不刷新），亮度经 powerd 设。
-- 用的时候把 Kindle 自带的「自动亮度」关掉，免得普通模式下两边一起调。
--
-- 做法：亮着灯时每 60 秒、以及每次唤醒，读一次光照，按曲线算目标亮度（加上用户偏好的偏移），和现在差 2 档以上才改。
-- 用户手动调了亮度（和上次自动设的不一样）→ 记下偏移，以后的自动值都加上它。灯关着不动，不替用户开灯。
-- 只在 Kindle、读得到光照（芯片或 alsLux）时启用；别的设备上自己隐藏。

local Device = require("device")
local UIManager = require("ui/uimanager")
local WidgetContainer = require("ui/widget/container/widgetcontainer")
local logger = require("logger")
local _ = require("gettext")
local T = require("ffi/util").template

local INTERVAL_S = 60
local MIN_CHANGE = 2

local function powerd()
    local ok, p = pcall(Device.getPowerDevice, Device)
    return ok and p or nil
end

-- 光线传感器芯片在 iio 下的读数文件（PW12 是 opt3001，/sys/bus/iio/devices/iio:device1/in_illuminance_input，单位 lux，带小数）。
-- 每次读驱动都让芯片测一次，不靠 powerd：2026-10-02 真机诊断，亚马逊界面在跑时 powerd 的 alsLux 和它同步（遮住 0～1、放开 28、亮处 744），
-- 但 KOReader 独占时 alsLux 不再刷新（一直停在 24），芯片的读数照样是实时的。按名字里有 in_illuminance_input 的设备找，不写死编号。
local function find_iio_lux()
    local lfs = require("libs/libkoreader-lfs")
    local base = "/sys/bus/iio/devices"
    local ok, iter, dir = pcall(lfs.dir, base)
    if not ok then
        return nil
    end
    for entry in iter, dir do
        local path = base .. "/" .. entry .. "/in_illuminance_input"
        if entry:sub(1, 1) ~= "." and lfs.attributes(path, "mode") == "file" then
            return path
        end
    end
    return nil
end

local IIO_LUX = find_iio_lux()

local function read_iio(path)
    local f = path and io.open(path, "r")
    if not f then
        return nil
    end
    local v = tonumber(f:read("*l") or "")
    f:close()
    if v and v >= 0 then
        return v
    end
    return nil
end

--- 当前光照（lux）：先读芯片，读不到退回 powerd 的 alsLux（独占时可能是旧值）。返回 (lux, 来源)。
local function read_lux(p)
    local v = read_iio(IIO_LUX)
    if v then
        return v, "sensor"
    end
    if not (p and p.lipc_handle) then
        return nil
    end
    local ok, lux = pcall(function()
        return p.lipc_handle:get_int_property("com.lab126.powerd", "alsLux")
    end)
    if ok and type(lux) == "number" and lux >= 0 then
        return lux, "powerd"
    end
    return nil
end

if not (Device:isKindle() and Device:hasFrontlight() and read_lux(powerd())) then
    return { disabled = true }
end

local AutoLight = WidgetContainer:extend{
    name = "autolight",
}

--- 光照（lux）→ 基准亮度（powerd 的档位，PW12 是 0–24）：两头低、中间高（用户 2026-10-02：墨水屏靠反射环境光，
--- 越亮前光越没用，不该像手机屏那样越亮越开大）。全黑时眼睛适应了黑暗，也要低；昏暗室内纸面发灰，补光最有用。
--- 节点之间按 log10(lux) 线性插值。亮处只降到最低档、不关灯（灯关着插件就不动，关了回到暗处就不会再开）。
local CURVE = {
    { 0, 4 },     -- 全黑
    { 3, 6 },     -- 很暗
    { 30, 10 },   -- 昏暗室内（Kindle 自带的自动亮度 28 lux 时设 9 档，2026-10-02 真机）
    { 150, 9 },   -- 普通室内
    { 500, 5 },   -- 明亮室内
    { 2000, 1 },  -- 窗边、户外
}

function AutoLight:curve(lux)
    local x = math.log10(lux + 1)
    local prev = CURVE[1]
    if x <= math.log10(prev[1] + 1) then
        return prev[2]
    end
    for i = 2, #CURVE do
        local cur = CURVE[i]
        local x0, x1 = math.log10(prev[1] + 1), math.log10(cur[1] + 1)
        if x <= x1 then
            return prev[2] + (cur[2] - prev[2]) * (x - x0) / (x1 - x0)
        end
        prev = cur
    end
    return prev[2]
end

function AutoLight:init()
    self.enabled = G_reader_settings:nilOrTrue("autolight_enabled")
    self.offset = G_reader_settings:readSetting("autolight_offset", 0)
    self.last_set = nil
    self.ui.menu:registerToMainMenu(self)
    if self.enabled then
        self:schedule(1)
    end
end

function AutoLight:schedule(delay)
    UIManager:unschedule(self.tick)
    if self.enabled then
        self.tick = self.tick or function() self:adjust() end
        UIManager:scheduleIn(delay or INTERVAL_S, self.tick)
    end
end

function AutoLight:target(p, lux)
    local v = math.floor(self:curve(lux) + self.offset + 0.5)
    return math.max(p.fl_min + 1, math.min(p.fl_max, v))
end

function AutoLight:adjust()
    local p = powerd()
    if self.enabled and p and p:isFrontlightOn() then
        local lux = read_lux(p)
        local cur = p:frontlightIntensity()
        if lux then
            -- 和上次自动设的不一样 = 用户手动调过：把差值记成偏好
            if self.last_set and cur ~= self.last_set then
                self.offset = self.offset + (cur - self.last_set)
                G_reader_settings:saveSetting("autolight_offset", self.offset)
                logger.dbg("AutoLight: 手动调过亮度", self.last_set, "→", cur, "偏移", self.offset)
            end
            local want = self:target(p, lux)
            if math.abs(want - cur) >= MIN_CHANGE then
                p:setIntensity(want)
                cur = p:frontlightIntensity()
                logger.dbg("AutoLight: lux", lux, "亮度", cur)
            end
            self.last_set = cur
        end
    end
    self:schedule()
end

function AutoLight:onResume()
    -- 唤醒时 KOReader 先恢复它记着的亮度，等一下再按当前光照调；记账从这次重新开始，不把恢复动作当成手动调
    self.last_set = nil
    self:schedule(2)
end

function AutoLight:onSuspend()
    UIManager:unschedule(self.tick)
end

function AutoLight:onCloseWidget()
    UIManager:unschedule(self.tick)
end

function AutoLight:addToMainMenu(menu_items)
    menu_items.autolight = {
        text = _("自动前光"),
        sorting_hint = "more_tools",
        sub_item_table = {
            {
                text = _("按光线自动调前光"),
                checked_func = function() return self.enabled end,
                callback = function()
                    self.enabled = not self.enabled
                    G_reader_settings:saveSetting("autolight_enabled", self.enabled)
                    self.last_set = nil
                    if self.enabled then self:schedule(1) else UIManager:unschedule(self.tick) end
                end,
            },
            {
                text_func = function()
                    local p = powerd()
                    local lux, src = read_lux(p)
                    return T(_("现在：光照 %1 lux（%2），亮度 %3（自动值 %4）"), lux and string.format("%.1f", lux) or "?",
                        src == "sensor" and _("传感器") or src or "?",
                        p and p:frontlightIntensity() or "?", (p and lux) and self:target(p, lux) or "?")
                end,
                keep_menu_open = true,
                callback = function(touchmenu_instance) touchmenu_instance:updateItems() end,
            },
            {
                text_func = function() return T(_("偏好偏移：%1（手动调亮度时自动记下）"), self.offset) end,
                keep_menu_open = true,
                callback = function(touchmenu_instance)
                    self.offset = 0
                    G_reader_settings:saveSetting("autolight_offset", 0)
                    self.last_set = nil
                    self:schedule(1)
                    touchmenu_instance:updateItems()
                end,
                help_text = _("点一下清零。"),
            },
        },
    }
end

return AutoLight
