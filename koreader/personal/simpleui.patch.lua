-- SimpleUI 插件的设置（settings/simpleui/sui_settings.lua 补丁；插件 simpleui.koplugin v2.7.1，MIT，
-- https://github.com/doctorhetfield-cmd/simpleui.koplugin）。属于个人设置：两台统一，--uninstall 不撤。
-- 键名按插件源码核过（modules/module_clock.lua、infra/sui_config.lua）。主页布局等你在掌阅上调好后收进来（用户 2026-09-29）。
return {
    -- 省电：不要每分钟刷新的时钟（用户 2026-09-29）。停在主页、书库时时钟每分钟重画一次 = 墨水屏每分钟局部刷新、唤醒一次处理器。
    -- 主页的时钟模块关掉：
    ["simpleui_hs_clock_enabled"] = false,
    -- 顶栏去掉时钟，只留 Wi-Fi、电池（整张写全：只写 clock 一项的话，插件会把没写的电池、Wi-Fi 也当隐藏）
    ["simpleui_topbar_config"] = {
        ["side"] = { ["clock"] = "hidden", ["wifi"] = "right", ["battery"] = "right" },
        ["order_left"] = {},
        ["order_right"] = { "wifi", "battery" },
    },
}
