-- 高级默认值（defaults.custom.lua 补丁；KOReader 启动时读它覆盖 defaults.lua 里的常量，源码 frontend/luadefaults.lua）。
return {
    -- 图标缩小（用户 2026-10-02）：菜单顶部的分类图标、标题栏、按钮、阅读时底部设置面板的图标都按它算，缺省 40 → 32（小两成）。
    ["DGENERIC_ICON_SIZE"] = 32,
}
