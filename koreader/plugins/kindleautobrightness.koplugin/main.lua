-- luacheck: globals G_reader_settings

local Device = require("device")
local logger = require("logger")
local _ = require("gettext")

local SETTING = "kindleautobrightness_enabled"
local WARMTH_SETTING = "kindleautobrightness_warmth_enabled"
local WRAPPER_STATE = "_kindle_auto_brightness_bridge_state"
local WARMTH_WRAPPER_STATE = "_kindle_auto_warmth_bridge_state"

local function valid_intensity(powerd, intensity)
    return type(intensity) == "number"
        and intensity == intensity
        and type(powerd.fl_min) == "number"
        and type(powerd.fl_max) == "number"
        and powerd.fl_min < powerd.fl_max
        and intensity >= powerd.fl_min
        and intensity <= powerd.fl_max
end

local function valid_warmth(warmth)
    return type(warmth) == "number"
        and warmth == warmth
        and warmth >= 0
        and warmth <= 100
end

local function has_capability(device, name)
    local method = device and device[name]
    if type(method) ~= "function" then
        return false
    end
    local ok, value = pcall(method, device)
    return ok and value == true
end

local function get_powerd(device)
    if not device or type(device.getPowerDevice) ~= "function" then
        return nil
    end
    local ok, powerd = pcall(device.getPowerDevice, device)
    if not ok or type(powerd) ~= "table" then
        return nil
    end
    return powerd
end

local function has_live_read(powerd)
    return type(powerd.frontlightIntensityHW) == "function"
end

local function has_warmth_reader(device, powerd)
    return has_capability(device, "hasNaturalLight")
        and type(powerd and powerd.frontlightWarmthHW) == "function"
end

local function has_native_light_sensor(powerd)
    local lipc_handle = powerd and powerd.lipc_handle
    if lipc_handle == nil then
        return false
    end
    local ok, lux = pcall(function()
        return lipc_handle:get_int_property("com.lab126.powerd", "alsLux")
    end)
    return ok and type(lux) == "number" and lux >= 0
end

local capability_powerd = get_powerd(Device)
local has_sensor = has_capability(Device, "hasLightSensor")
    or has_native_light_sensor(capability_powerd)
if not has_capability(Device, "isKindle")
        or not has_capability(Device, "hasFrontlight")
        or not has_sensor
        or not capability_powerd
        or not has_live_read(capability_powerd) then
    return { disabled = true }
end

local WidgetContainer = require("ui/widget/container/widgetcontainer")

local KindleAutoBrightness = WidgetContainer:extend{
    name = "kindleautobrightness",
    settings_key = SETTING,
}

local function read_live_intensity(powerd)
    if type(powerd.frontlightIntensityHW) ~= "function" then
        return nil, "reader unavailable"
    end

    local ok, intensity = pcall(powerd.frontlightIntensityHW, powerd)
    if not ok then
        return nil, "reader error: " .. tostring(intensity)
    end
    if not valid_intensity(powerd, intensity) then
        return nil, "reader returned an invalid intensity"
    end
    return intensity
end

local function read_live_warmth(powerd)
    if type(powerd.frontlightWarmthHW) ~= "function" then
        return nil, "reader unavailable"
    end

    local ok, warmth = pcall(powerd.frontlightWarmthHW, powerd)
    if not ok then
        return nil, "reader error: " .. tostring(warmth)
    end
    if not valid_warmth(warmth) then
        return nil, "reader returned an invalid warmth"
    end
    return warmth
end

local function fallback_intensity(powerd, original_ok, original_result)
    if original_ok and valid_intensity(powerd, original_result) then
        return original_result
    end
    if valid_intensity(powerd, powerd.fl_intensity) then
        return powerd.fl_intensity
    end
    return 0
end

local function make_wrapper(state)
    return function(this, ...)
        local original_ok, original_result = pcall(state.original_method, this, ...)
        if not original_ok then
            logger.dbg("KindleAutoBrightness: original frontlightIntensity failed", original_result)
        end

        local live_intensity, read_error = read_live_intensity(this)
        if live_intensity == nil then
            logger.dbg("KindleAutoBrightness: live frontlight read unavailable", read_error)
            return fallback_intensity(this, original_ok, original_result)
        end

        -- A minimum/off read is a state observation, not a new remembered level.
        if live_intensity <= this.fl_min then
            this.is_fl_on = false
            return 0
        end

        this.fl_intensity = live_intensity
        this.is_fl_on = true
        return live_intensity
    end
end

local function fallback_warmth(powerd, original_ok, original_result)
    if original_ok and valid_warmth(original_result) then
        return original_result
    end
    if valid_warmth(powerd.fl_warmth) then
        return powerd.fl_warmth
    end
    return 0
end

local function make_warmth_wrapper(state)
    return function(this, ...)
        local original_ok, original_result = pcall(state.original_method, this, ...)
        if not original_ok then
            logger.dbg("KindleAutoBrightness: original frontlightWarmth failed", original_result)
        end

        local live_warmth, read_error = read_live_warmth(this)
        if live_warmth == nil then
            logger.dbg("KindleAutoBrightness: live frontlight warmth read unavailable", read_error)
            return fallback_warmth(this, original_ok, original_result)
        end

        this.fl_warmth = live_warmth
        return live_warmth
    end
end

function KindleAutoBrightness:_ensureWrapper()
    local current_powerd = get_powerd(self.device or Device)
    if not current_powerd then
        return false
    end

    local existing = current_powerd[WRAPPER_STATE]
    if existing and existing.wrapper and current_powerd.frontlightIntensity == existing.wrapper then
        self.powerd = current_powerd
        self.wrapper_state = existing
        return true
    end
    if existing then
        if current_powerd.frontlightIntensity == existing.original_method then
            current_powerd[WRAPPER_STATE] = nil
        else
            logger.dbg("KindleAutoBrightness: refusing to replace another frontlight wrapper")
            return false
        end
    end

    local original_method = current_powerd.frontlightIntensity
    if type(original_method) ~= "function" then
        return false
    end

    local state = {
        original_method = original_method,
        original_raw_method = rawget(current_powerd, "frontlightIntensity"),
    }
    state.wrapper = make_wrapper(state)
    current_powerd[WRAPPER_STATE] = state
    current_powerd.frontlightIntensity = state.wrapper
    self.powerd = current_powerd
    self.wrapper_state = state
    return true
end

function KindleAutoBrightness:_restoreWrapper(powerd_to_restore)
    local current_powerd = powerd_to_restore or self.powerd or get_powerd(self.device or Device)
    if not current_powerd then
        return false
    end

    local state = current_powerd[WRAPPER_STATE]
    if not state then
        self.wrapper_state = nil
        return true
    end

    if current_powerd.frontlightIntensity ~= state.wrapper then
        logger.dbg("KindleAutoBrightness: leaving a changed frontlight method untouched")
        return false
    end

    if state.original_raw_method == nil then
        current_powerd.frontlightIntensity = nil
    else
        current_powerd.frontlightIntensity = state.original_raw_method
    end
    current_powerd[WRAPPER_STATE] = nil
    self.wrapper_state = nil
    return true
end

function KindleAutoBrightness:_ensureWarmthWrapper()
    local current_powerd = get_powerd(self.device or Device)
    if not current_powerd or not has_warmth_reader(self.device or Device, current_powerd) then
        return false
    end

    local existing = current_powerd[WARMTH_WRAPPER_STATE]
    if existing and existing.wrapper and current_powerd.frontlightWarmth == existing.wrapper then
        self.warmth_powerd = current_powerd
        self.warmth_wrapper_state = existing
        return true
    end
    if existing then
        if current_powerd.frontlightWarmth == existing.original_method then
            current_powerd[WARMTH_WRAPPER_STATE] = nil
        else
            logger.dbg("KindleAutoBrightness: refusing to replace another frontlight warmth wrapper")
            return false
        end
    end

    local original_method = current_powerd.frontlightWarmth
    if type(original_method) ~= "function" then
        return false
    end

    local state = {
        original_method = original_method,
        original_raw_method = rawget(current_powerd, "frontlightWarmth"),
    }
    state.wrapper = make_warmth_wrapper(state)
    current_powerd[WARMTH_WRAPPER_STATE] = state
    current_powerd.frontlightWarmth = state.wrapper
    self.warmth_powerd = current_powerd
    self.warmth_wrapper_state = state
    return true
end

function KindleAutoBrightness:_restoreWarmthWrapper(powerd_to_restore)
    local current_powerd = powerd_to_restore or self.warmth_powerd or get_powerd(self.device or Device)
    if not current_powerd then
        return false
    end

    local state = current_powerd[WARMTH_WRAPPER_STATE]
    if not state then
        self.warmth_powerd = nil
        self.warmth_wrapper_state = nil
        return true
    end

    if current_powerd.frontlightWarmth ~= state.wrapper then
        logger.dbg("KindleAutoBrightness: leaving a changed frontlight warmth method untouched")
        return false
    end

    if state.original_raw_method == nil then
        current_powerd.frontlightWarmth = nil
    else
        current_powerd.frontlightWarmth = state.original_raw_method
    end
    current_powerd[WARMTH_WRAPPER_STATE] = nil
    self.warmth_powerd = nil
    self.warmth_wrapper_state = nil
    return true
end

function KindleAutoBrightness:_setEnabled(enabled)
    enabled = enabled == true
    if enabled then
        local installed = self:_ensureWrapper()
        self.enabled = installed
    else
        self:_restoreWrapper()
        self.enabled = false
    end
    G_reader_settings:saveSetting(SETTING, self.enabled)
    return self.enabled
end

function KindleAutoBrightness:_setWarmthEnabled(enabled)
    enabled = enabled == true
    if enabled then
        local current_powerd = get_powerd(self.device or Device)
        self.warmth_supported = has_warmth_reader(self.device or Device, current_powerd)
        if self.warmth_supported then
            self.warmth_enabled = self:_ensureWarmthWrapper()
        else
            self.warmth_enabled = false
        end
    else
        self:_restoreWarmthWrapper()
        self.warmth_enabled = false
    end
    G_reader_settings:saveSetting(WARMTH_SETTING, self.warmth_enabled)
    return self.warmth_enabled
end

function KindleAutoBrightness:init()
    self.device = Device
    self.warmth_supported = has_warmth_reader(self.device, get_powerd(self.device))
    local stored = G_reader_settings:readSetting(SETTING)
    if stored == nil then
        self.enabled = false
        G_reader_settings:saveSetting(SETTING, false)
    else
        self.enabled = G_reader_settings:isTrue(SETTING)
    end

    local stored_warmth = G_reader_settings:readSetting(WARMTH_SETTING)
    if self.warmth_supported and stored_warmth ~= nil then
        self.warmth_enabled = G_reader_settings:isTrue(WARMTH_SETTING)
    else
        self.warmth_enabled = false
        G_reader_settings:saveSetting(WARMTH_SETTING, false)
    end

    if self.ui and self.ui.menu then
        self.ui.menu:registerToMainMenu(self)
    end

    if self.enabled then
        if not self:_ensureWrapper() then
            self.enabled = false
            G_reader_settings:saveSetting(SETTING, false)
        end
    else
        self:_restoreWrapper()
    end

    if self.warmth_enabled then
        if not self:_ensureWarmthWrapper() then
            self.warmth_enabled = false
            G_reader_settings:saveSetting(WARMTH_SETTING, false)
        end
    else
        self:_restoreWarmthWrapper()
    end
end

function KindleAutoBrightness:onResume()
    if self.enabled and not self:_ensureWrapper() then
        self.enabled = false
        G_reader_settings:saveSetting(SETTING, false)
    end
    if self.warmth_enabled and not self:_ensureWarmthWrapper() then
        self.warmth_enabled = false
        G_reader_settings:saveSetting(WARMTH_SETTING, false)
    end
end

function KindleAutoBrightness:addToMainMenu(menu_items)
    menu_items.kindleautobrightness = {
        text = _("Synchronize with Kindle Auto Brightness"),
        sorting_hint = "more_tools",
        help_text = _([[
This external plugin requires native Kindle Auto Brightness to be enabled first.
It makes KOReader's brightness controls read Amazon's current hardware brightness; it does not
implement its own ambient light sensor (ALS) algorithm.]]),
        checked_func = function()
            return self.enabled
        end,
        check_callback_updates_menu = true,
        callback = function(touchmenu_instance)
            self:_setEnabled(not self.enabled)
            if touchmenu_instance then
                touchmenu_instance:updateItems()
            end
        end,
    }
    if self.warmth_supported then
        menu_items.kindleautobrightness_warmth = {
            text = _("Synchronize with Kindle scheduled warmth"),
            sorting_hint = "more_tools",
            help_text = _([[
This makes KOReader's warmth controls read the actual current Kindle hardware warmth on demand,
including changes made by Kindle's schedule. It does not choose or run a schedule.
If KOReader AutoWarmth is also enabled, the two schedulers remain independent; do not expect them
to cooperate as one algorithm.]]),
            checked_func = function()
                return self.warmth_enabled
            end,
            check_callback_updates_menu = true,
            callback = function(touchmenu_instance)
                self:_setWarmthEnabled(not self.warmth_enabled)
                if touchmenu_instance then
                    touchmenu_instance:updateItems()
                end
            end,
        }
    end
end

return KindleAutoBrightness
