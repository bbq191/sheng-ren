-- 按太阳调前光（sheng-ren 的 koreader/plugins/sunlight.koplugin，用户 2026-10-02 要）：按所在地（默认昆明）的太阳高度分档，
-- 自动设掌阅前光的冷光和暖光。
--
-- 为什么自己写：KOReader 安卓版的前光驱动里没有掌阅，暖光调不了，自带的「自动色温」在掌阅上没用（它也只管暖光、不管亮度）；
-- 掌阅没有光线传感器。掌阅自己的系统服务 ireader（android.os.IIreaderManager）有 setColdBrightness / setWarmBrightness，
-- 参数是两路灯的原始值 0–255（冷光 lm3630a_ledb、暖光 lm3630a_leda），普通应用就能调（2026-10-02 真机：事务号 39/40 从掌阅
-- framework.jar 的 Stub 读出来，root 的 adb 和普通应用都实测有效）。这里经 JNI 拿 binder、直接 transact。
--
-- 做法：亮着屏时每 2 分钟、以及每次唤醒，按太阳高度定档；进了新的一档就把灯设成这一档的值。
-- 完全接管（用户 2026-10-02 定，和 Kindle autolight 一致）：每次检查都把灯拉回当前太阳档，灯的实际值和当前档不一样（含被控制中心手动改过）就重设；想一直手动就关掉本插件。
-- 太阳高度用天文年历的低精度公式（精度约 0.01°，和公开数据对过：昆明 2026-10-02 日出 07:02、日落 18:56），只要经纬度，不用时区。
-- 只在找得到掌阅这两路灯的安卓设备上启用；别的设备上自己隐藏。

local Device = require("device")
local UIManager = require("ui/uimanager")
local WidgetContainer = require("ui/widget/container/widgetcontainer")
local logger = require("logger")
local _ = require("gettext")
local T = require("ffi/util").template

local COLD_NODE = "/sys/class/backlight/lm3630a_ledb/actual_brightness"
local WARM_NODE = "/sys/class/backlight/lm3630a_leda/actual_brightness"
local DESCRIPTOR = "android.os.IIreaderManager"
local TX_SET_COLD = 39
local TX_SET_WARM = 40
local INTERVAL_S = 120
local TOLERANCE = 2 -- 读回值和设的值差这么多以内算没动过

local function read_node(path)
    local f = io.open(path, "r")
    if not f then
        return nil
    end
    local v = tonumber(f:read("*l") or "")
    f:close()
    return v
end

if not (Device:isAndroid() and read_node(COLD_NODE) and read_node(WARM_NODE)) then
    return { disabled = true }
end

local DEFAULT_LOCATION = { name = "昆明", lat = 25.04, lon = 102.71 }

--- 档位：太阳高度（度）≥ min 的最后一档生效，从低到高排。值是灯的原始值 0–255（掌阅控制中心最亮约 210）。
--- 起点是估的，用「把现在的灯存为本档」按自己的眼睛调。
local DEFAULT_STEPS = {
    -- 冷光(亮度)两头低中间高：深夜/白天低、晨昏(昏暗)最高（墨水屏靠反射光，越暗越要补光）；暖光夜→昼单调递减（夜里最暖、白天归零）。
    -- 值是灯的原始值 0–255（控制中心最亮约 210），对比比初版拉大、用上更大范围；冷光最低有效约 5–8。起点是估的，用「把现在的灯存为本档」按眼睛调。
    { min = -90, name = "深夜", cold = 25, warm = 90 },   -- 天文昏影终以后（太阳在地平线下 18° 以下）
    { min = -18, name = "夜", cold = 55, warm = 80 },     -- 天文、航海晨昏
    { min = -6, name = "晨昏", cold = 95, warm = 55 },    -- 民用晨昏：天还有点亮，最暗、补光最有用 → 冷光最高
    { min = -0.833, name = "日出日落", cold = 55, warm = 25 }, -- 太阳在地平线上 10° 以内
    { min = 10, name = "白天", cold = 18, warm = 0 },     -- 环境够亮，前光降下来、不加暖
}

local function copy_steps(steps)
    local out = {}
    for i, s in ipairs(steps) do
        out[i] = { min = s.min, name = s.name, cold = s.cold, warm = s.warm }
    end
    return out
end

--- 太阳高度（度）。t：Unix 时间（秒，UTC），lat/lon：纬度、东经（度）。
local function sun_elevation(t, lat, lon)
    local n = t / 86400 + 2440587.5 - 2451545.0 -- 距 J2000.0 的日数
    local L = (280.460 + 0.9856474 * n) % 360 -- 平黄经
    local g = math.rad((357.528 + 0.9856003 * n) % 360) -- 平近点角
    local lambda = math.rad(L + 1.915 * math.sin(g) + 0.020 * math.sin(2 * g)) -- 黄经
    local eps = math.rad(23.439 - 0.0000004 * n) -- 黄赤交角
    local dec = math.asin(math.sin(eps) * math.sin(lambda)) -- 赤纬
    local ra = math.atan2(math.cos(eps) * math.sin(lambda), math.cos(lambda)) -- 赤经
    local gmst = math.rad((280.46061837 + 360.98564736629 * n) % 360) -- 格林尼治恒星时
    local ha = gmst + math.rad(lon) - ra -- 时角
    local phi = math.rad(lat)
    return math.deg(math.asin(math.sin(phi) * math.sin(dec) + math.cos(phi) * math.cos(dec) * math.cos(ha)))
end

--- 经 JNI 调 IIreaderManager 的一个 (int) 方法。成功返回 true。
local function ireader_call(code, value)
    local ok, res = pcall(function()
        local android = require("android")
        local ffi = require("ffi")
        return android.jni:context(android.app.activity.vm, function(jni)
            local env = jni.env
            local function clear()
                if env[0].ExceptionCheck(env) ~= 0 then
                    env[0].ExceptionClear(env)
                    return true
                end
                return false
            end
            local name = env[0].NewStringUTF(env, "ireader")
            local binder = jni:callStaticObjectMethod("android/os/ServiceManager", "getService",
                "(Ljava/lang/String;)Landroid/os/IBinder;", name)
            env[0].DeleteLocalRef(env, name)
            if clear() or binder == nil then
                return false
            end
            local data = jni:callStaticObjectMethod("android/os/Parcel", "obtain", "()Landroid/os/Parcel;")
            local reply = jni:callStaticObjectMethod("android/os/Parcel", "obtain", "()Landroid/os/Parcel;")
            local desc = env[0].NewStringUTF(env, DESCRIPTOR)
            jni:callVoidMethod(data, "writeInterfaceToken", "(Ljava/lang/String;)V", desc)
            jni:callVoidMethod(data, "writeInt", "(I)V", ffi.new("int32_t", value))
            local sent = jni:callBooleanMethod(binder, "transact", "(ILandroid/os/Parcel;Landroid/os/Parcel;I)Z",
                ffi.new("int32_t", code), data, reply, ffi.new("int32_t", 0))
            local failed = clear()
            if not failed then
                jni:callVoidMethod(reply, "readException", "()V") -- 服务端抛了异常（比如没权限）会在这里重新抛出
                failed = clear()
            end
            jni:callVoidMethod(data, "recycle", "()V")
            jni:callVoidMethod(reply, "recycle", "()V")
            clear()
            for _, ref in ipairs({ desc, data, reply, binder }) do
                env[0].DeleteLocalRef(env, ref)
            end
            return sent and not failed
        end)
    end)
    if not ok then
        logger.warn("SunLight: 调 ireader 服务出错", res)
        return false
    end
    return res
end

local SunLight = WidgetContainer:extend{
    name = "sunlight",
}

function SunLight:init()
    self.enabled = G_reader_settings:nilOrTrue("sunlight_enabled")
    self.location = G_reader_settings:readSetting("sunlight_location") or DEFAULT_LOCATION
    self.steps = G_reader_settings:readSetting("sunlight_steps") or copy_steps(DEFAULT_STEPS)
    self.last_set_step = nil -- 上次设的档位序号（只为少写日志；完全接管下每次都会按需重设）
    self.failed = false
    self.ui.menu:registerToMainMenu(self)
    if self.enabled then
        self:schedule(1)
    end
end

function SunLight:elevation(t)
    return sun_elevation(t or os.time(), self.location.lat, self.location.lon)
end

function SunLight:stepIndex(elev)
    local idx = 1
    for i, s in ipairs(self.steps) do
        if elev >= s.min then
            idx = i
        end
    end
    return idx
end

--- 下一次换档的时间和档位（往后按分钟找，最多 24 小时）。
function SunLight:nextChange()
    local now = os.time()
    local cur = self:stepIndex(self:elevation(now))
    for m = 1, 24 * 60 do
        local t = now + m * 60
        local idx = self:stepIndex(self:elevation(t))
        if idx ~= cur then
            return t, idx
        end
    end
    return nil
end

function SunLight:schedule(delay)
    UIManager:unschedule(self.tick)
    if self.enabled then
        self.tick = self.tick or function() self:adjust() end
        UIManager:scheduleIn(delay or INTERVAL_S, self.tick)
    end
end

function SunLight:setLights(cold, warm)
    local ok = ireader_call(TX_SET_COLD, cold) and ireader_call(TX_SET_WARM, warm)
    self.failed = not ok
    return ok
end

local function near(a, b)
    return a and b and math.abs(a - b) <= TOLERANCE
end

function SunLight:adjust()
    if not self.enabled then
        return
    end
    local idx = self:stepIndex(self:elevation())
    local s = self.steps[idx]
    local cold, warm = read_node(COLD_NODE), read_node(WARM_NODE)
    -- 完全接管：灯和当前档不一致（含手动改过、屏保余留的 0）就拉回当前档
    if not (near(cold, s.cold) and near(warm, s.warm)) then
        if self:setLights(s.cold, s.warm) and self.last_set_step ~= idx then
            logger.dbg("SunLight: 档位", s.name, "冷", s.cold, "暖", s.warm)
            self.last_set_step = idx
        end
    end
    self:schedule()
end

function SunLight:onResume()
    -- 唤醒后按当前太阳档重设（屏保休眠会把灯关成 0/0）
    self.last_set_step = nil
    self:schedule(2)
end

function SunLight:onSuspend()
    UIManager:unschedule(self.tick)
end

function SunLight:onCloseWidget()
    UIManager:unschedule(self.tick)
end

function SunLight:saveSteps()
    G_reader_settings:saveSetting("sunlight_steps", self.steps)
end

function SunLight:addToMainMenu(menu_items)
    menu_items.sunlight = {
        text = _("按太阳调前光"),
        sorting_hint = "more_tools",
        sub_item_table = {
            {
                text = _("按太阳高度自动调冷光和暖光"),
                checked_func = function() return self.enabled end,
                callback = function()
                    self.enabled = not self.enabled
                    G_reader_settings:saveSetting("sunlight_enabled", self.enabled)
                    self.last_set_step = nil
                    if self.enabled then self:schedule(1) else UIManager:unschedule(self.tick) end
                end,
            },
            {
                text_func = function()
                    local elev = self:elevation()
                    local s = self.steps[self:stepIndex(elev)]
                    local state = self.failed and _("（调灯失败）") or ""
                    return T(_("现在：太阳 %1°，%2档（冷 %3 暖 %4），灯 冷 %5 暖 %6%7"), string.format("%.1f", elev),
                        s.name, s.cold, s.warm, read_node(COLD_NODE) or "?", read_node(WARM_NODE) or "?", state)
                end,
                keep_menu_open = true,
                callback = function(touchmenu_instance) touchmenu_instance:updateItems() end,
            },
            {
                text_func = function()
                    local t, idx = self:nextChange()
                    if not t then
                        return _("下一档：24 小时内不换档")
                    end
                    return T(_("下一档：%1 %2（%3：%4）"), os.date("%H:%M", t), self.steps[idx].name,
                        self.location.name, string.format("%.2f, %.2f", self.location.lat, self.location.lon))
                end,
                keep_menu_open = true,
                callback = function(touchmenu_instance) touchmenu_instance:updateItems() end,
            },
            {
                text = _("把现在的灯存为本档"),
                keep_menu_open = true,
                callback = function(touchmenu_instance)
                    local idx = self:stepIndex(self:elevation())
                    local cold, warm = read_node(COLD_NODE), read_node(WARM_NODE)
                    if cold and warm then
                        self.steps[idx].cold, self.steps[idx].warm = cold, warm
                        self:saveSteps()
                        self.last_set_step = nil
                    end
                    touchmenu_instance:updateItems()
                end,
                help_text = _("先在控制中心把亮度、色温调到合适，再点这里：以后到这一档就用这个值。"),
            },
            {
                text = _("档位恢复默认"),
                keep_menu_open = true,
                callback = function(touchmenu_instance)
                    self.steps = copy_steps(DEFAULT_STEPS)
                    G_reader_settings:delSetting("sunlight_steps")
                    self.last_set_step = nil
                    self:schedule(1)
                    touchmenu_instance:updateItems()
                end,
            },
        },
    }
end

return SunLight
