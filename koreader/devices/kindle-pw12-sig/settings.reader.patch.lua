-- Kindle 专属（settings.reader.lua 补丁）。个人设置见 ../../personal/，这里只放随设备不同的。
return {
    -- 状态栏字体：京華老宋体，用 Kindle 上的 v3.0（字体族名 KingHwaOldSong；掌阅上是 v2.0 的另一个文件，用户 2026-09-28 选定）
    ["footer"] = { ["text_font_face"] = "./fonts/京華老宋体v3.0.ttf" },
    -- 阅读背景：再生纸纹理（koreader/backgrounds/，apply.sh 拷到设备的 backgrounds/；用户 2026-09-29 要）。KOReader 菜单里没有这个设置，
    -- 只能写这里；路径要写设备上的绝对路径，所以放在设备层。不想要：删掉这一行再 apply（或 --uninstall 撤方案）。
    ["cre_background_image"] = "/mnt/us/koreader/backgrounds/recycled-medium.png", -- 中档（用户 2026-09-29：浅的太淡）
    -- 感光插件（plugins/kindleautobrightness.koplugin）缺省关着，这里打开。要先在 Kindle 下拉菜单里开「自动亮度」再进 KOReader。
    -- 色温同步（kindleautobrightness_warmth_enabled）不开：和 KOReader 自己的自动色温插件同时开，两套日程各管各的（插件 README）。
    ["kindleautobrightness_enabled"] = true,
}
